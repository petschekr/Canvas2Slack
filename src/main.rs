use std::time::Duration;
use serde_json::json;
use xml::reader::{ EventReader, XmlEvent };

mod settings;

#[derive(PartialEq)]
enum ParseState {
    MetaData,
    Entry,
    Title,
    Published,
    Author,
    Content,
    Id,
}
#[derive(Debug)]
struct Entry {
    id: String,
    title: String,
    author: String,
    content: String,
    link: String,
    published: chrono::DateTime<chrono::Utc>,
}
impl std::default::Default for Entry {
    fn default() -> Self {
        Self {
            id: String::new(),
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
    let db = sled::open("cache.db").unwrap();

    let slack = slack_api::requests::default_client().unwrap();
    let posting_channel = slack_api::channels::list(&slack, &settings.bot_token, &Default::default())
        .unwrap()
        .channels
        .unwrap()
        .into_iter()
        .find(|c| c.name.as_ref().unwrap() == &settings.channel_name)
        .expect("Workspace does not contain specified channel to post in");

    loop {
        let feed_content = client
            .get(&settings.feed_url)
            .send();
        let feed_content = match feed_content {
            Ok(mut feed_content) => feed_content.text().unwrap(),
            Err(err) => {
                println!("Error getting feed: {:#?}", err);
                std::thread::sleep(Duration::from_secs(1));
                continue;
            }
        };

        let mut state = vec![ParseState::MetaData];
        let in_state = |state: &Vec<ParseState>, check_state: ParseState| {
            state.last().map(|s| s == &check_state).unwrap_or(false)
        };

        let parser = EventReader::new(feed_content.as_bytes());

        let mut _main_link = String::new();
        let mut entries = Vec::new();
        let mut entry: Entry = Default::default();
        let mut content = String::new();

        for event in parser {
            match event {
                Ok(XmlEvent::StartElement { name, attributes, .. }) => {
                    match name.local_name.as_ref() {
                        "entry" => state.push(ParseState::Entry),
                        "link" if in_state(&state, ParseState::MetaData) => _main_link = attributes.into_iter().find(|a| &a.name.local_name == "href").unwrap().value,
                        "link" if !in_state(&state, ParseState::MetaData) => entry.link = attributes.into_iter().find(|a| &a.name.local_name == "href").unwrap().value,
                        "title" if in_state(&state, ParseState::Entry) => state.push(ParseState::Title),
                        "published" => state.push(ParseState::Published),
                        "author" => state.push(ParseState::Author),
                        "content" => state.push(ParseState::Content),
                        "id" => state.push(ParseState::Id),
                        _ => {}
                    }
                }
                Ok(XmlEvent::Characters(chars)) => content = chars,
                Ok(XmlEvent::EndElement { name }) => {
                    match name.local_name.as_ref() {
                        "entry" => {
                            if !db.contains_key(&entry.id).unwrap() {
                                db.insert(&entry.id, "").unwrap();
                                entries.push(entry);
                            }
                            entry = Default::default();
                        },
                        "title" => entry.title = content.clone(),
                        "published" => entry.published = chrono::DateTime::parse_from_rfc3339(&content).unwrap().with_timezone(&chrono::Utc),
                        "author" => {
                            let mut names = content.split_whitespace();
                            entry.author = format!("{} {}", names.next().unwrap_or_default(), names.last().unwrap_or_default());
                        },
                        "content" => {
                            let mut rendered_text = String::new();
                            let html = scraper::Html::parse_fragment(&content);
                            let mut link_url: Option<&str> = None;
                            let mut skip_text = false;
                            for edge in html.root_element().traverse() {
                                match edge {
                                    ego_tree::iter::Edge::Open(element) => {
                                        match element.value() {
                                            scraper::Node::Element(element) => {
                                                rendered_text.push_str(match element.name() {
                                                    "strong" | "b" => "*",
                                                    "i" | "em" => "_",
                                                    "br" => "\n",
                                                    "a" if element.attr("href").is_some() => {
                                                        link_url = Some(element.attr("href").unwrap());
                                                        "<"
                                                    },
                                                    "table" => {
                                                        skip_text = true;
                                                        "*_See table on Canvas_*"
                                                    },
                                                    _ => "",
                                                });
                                            },
                                            scraper::Node::Text(text) => {
                                                if !skip_text {
                                                    let text = text.text.to_string();
                                                    if let Some(url) = link_url {
                                                        if url.starts_with("/") {
                                                            rendered_text.push_str("https://gatech.instructure.com")
                                                        }
                                                        rendered_text.push_str(url);
                                                        if url != &text {
                                                            rendered_text.push_str("|");
                                                            rendered_text.push_str(&text);
                                                        }
                                                        link_url = None;
                                                    }
                                                    else {
                                                        rendered_text.push_str(&text);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                    ego_tree::iter::Edge::Close(element) => {
                                        match element.value() {
                                            scraper::Node::Element(element) => {
                                                rendered_text.push_str(match element.name() {
                                                    "strong" | "b" => "*",
                                                    "i" | "em" => "_",
                                                    "p" => "\n",
                                                    "a" => ">",
                                                    "table" => {
                                                        skip_text = false;
                                                        ""
                                                    },
                                                    _ => "",
                                                });
                                            },
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            entry.content = rendered_text;
                        },
                        "id" => entry.id = content.clone(),
                        _ => {}
                    };
                    state.pop();
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
                _ => {}
            }
        }

        for entry in entries.into_iter().rev() {
            slack_api::chat::post_message(&slack, &settings.bot_token, &slack_api::chat::PostMessageRequest {
                channel: posting_channel.id.as_ref().unwrap(),
                text: "<!channel>",
                attachments: Some(&json!([
                    {
                        "fallback": entry.title.trim(),
                        "color": "#EEB211",
                        "author_name": entry.author.trim(),
                        "title": entry.title.trim(),
                        "title_link": entry.link,
                        "text": entry.content.trim(),
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
