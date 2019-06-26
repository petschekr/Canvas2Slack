use xml::reader::{ EventReader, XmlEvent };
use std::time::Duration;
use serde_json::json;

mod settings;

#[derive(PartialEq)]
enum ParseState {
    MetaData,
    Entry,
    Title,
    Published,
    Author,
    Content,
}
#[derive(Debug)]
struct Entry {
    title: String,
    author: String,
    content: String,
    link: String,
    published: chrono::DateTime<chrono::Utc>,
}
impl std::default::Default for Entry {
    fn default() -> Self {
        Self {
            title: String::new(),
            author: String::new(),
            content: String::new(),
            link: String::new(),
            published: chrono::Utc::now(),
        }
    }
}

fn main() {
    let settings = settings::Settings::new();
    let interval = Duration::from_secs(settings.interval_sec);
    let client = reqwest::Client::new();

    let slack = slack_api::requests::default_client().unwrap();
    let posting_channel = slack_api::channels::list(&slack, &settings.bot_token, &Default::default())
        .unwrap()
        .channels
        .unwrap()
        .into_iter()
        .find(|c| c.name.as_ref().unwrap() == &settings.channel_name)
        .expect("Workspace does not contain specified channel to post in");

    let mut last_updated = chrono::Utc::now();

    loop {
        let feed_content = client
            .get(&settings.feed_url)
            .send().unwrap()
            .text().unwrap();

        let mut state = vec![ParseState::MetaData];
        let in_state = |state: &Vec<ParseState>, check_state: ParseState| {
            state.last().map(|s| s == &check_state).unwrap_or(false)
        };

        let parser = EventReader::new(feed_content.as_bytes());

        let mut main_link = String::new();
        let mut entries = Vec::new();
        let mut entry: Entry = Default::default();
        let mut content = String::new();

        for event in parser {
            match event {
                Ok(XmlEvent::StartElement { name, attributes, .. }) => {
                    match name.local_name.as_ref() {
                        "entry" => state.push(ParseState::Entry),
                        "link" if in_state(&state, ParseState::MetaData) => main_link = attributes.into_iter().find(|a| &a.name.local_name == "href").unwrap().value,
                        "link" if !in_state(&state, ParseState::MetaData) => entry.link = attributes.into_iter().find(|a| &a.name.local_name == "href").unwrap().value,
                        "title" if in_state(&state, ParseState::Entry) => state.push(ParseState::Title),
                        "published" => state.push(ParseState::Published),
                        "author" => state.push(ParseState::Author),
                        "content" => state.push(ParseState::Content),
                        _ => {}
                    }
                }
                Ok(XmlEvent::Characters(chars)) => content = chars,
                Ok(XmlEvent::EndElement { name }) => {
                    match name.local_name.as_ref() {
                        "entry" => {
                            let diff = last_updated.signed_duration_since(entry.published).num_seconds();
                            if diff < 0 {
                                // Only add entries that we haven't seen yet
                                entries.push(entry);
                            }
                            entry = Default::default();
                        }
                        "title" => entry.title = content.clone(),
                        "published" => entry.published = chrono::DateTime::parse_from_rfc3339(&content).unwrap().with_timezone(&chrono::Utc),
                        "author" => entry.author = content.clone(),
                        "content" => {
                            let tag_stripper = EventReader::new(content.as_bytes());
                            let mut content: Vec<String> = Vec::new();
                            for event in tag_stripper {
                                if let Ok(XmlEvent::Characters(chars)) = event {
                                    content.push(chars);
                                }
                            }
                            entry.content = content.join("\n\n");
                        },
                        _ => {}
                    };
                    state.pop();
                }
                Err(e) => {
                    println!("Error: {}", e);
                    break;
                }
                _ => {}
            }
        }
        last_updated = chrono::Utc::now();

        for entry in entries.into_iter() {
            slack_api::chat::post_message(&slack, &settings.bot_token, &slack_api::chat::PostMessageRequest {
                channel: posting_channel.id.as_ref().unwrap(),
                text: "<!channel>",
                attachments: Some(&json!([
                    {
                        "fallback": entry.title,
                        "color": "#EEB211",
                        "author_name": entry.author,
                        "title": entry.title,
                        "title_link": entry.link,
                        "text": entry.content,
                        "footer": "via Canvas",
                        "ts": entry.published.timestamp()
                    }
                ]).to_string()),
                ..Default::default()
            }).unwrap();
            // Comply with Slack's rate limiting
            std::thread::sleep(Duration::from_secs(1));
        }

        std::thread::sleep(interval);
    }
}
