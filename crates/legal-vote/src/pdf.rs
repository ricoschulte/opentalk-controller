use anyhow::Result;
use controller::prelude::chrono::{DateTime, Utc};
use controller::prelude::*;
use database::{Db, DbConnection};
use db_storage::legal_votes::types::protocol::v1::{
    Cancel, ProtocolEntry, Start, StopKind, Vote, VoteEvent,
};
use db_storage::legal_votes::types::{
    CancelReason, FinalResults, Invalid, Parameters, UserParameters, VoteOption, Votes,
};
use db_storage::users::{User, UserId};
use genpdf::elements::*;
use genpdf::fonts::{FontData, FontFamily};
use genpdf::style::Style;
use genpdf::{Document, Element, SimplePageDecorator};
use itertools::Itertools;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;

/// Wrapper type to allow simple usage of generic `Element` implementors
struct BoxedElement {
    inner: Box<dyn Element>,
}

impl BoxedElement {
    fn new<T: Element + 'static>(element: T) -> Self {
        Self {
            inner: Box::new(element),
        }
    }
}

impl From<Box<dyn Element>> for BoxedElement {
    fn from(e: Box<dyn Element>) -> Self {
        Self { inner: e }
    }
}

impl Element for BoxedElement {
    fn render(
        &mut self,
        context: &genpdf::Context,
        area: genpdf::render::Area<'_>,
        style: genpdf::style::Style,
    ) -> Result<genpdf::RenderResult, genpdf::error::Error> {
        self.inner.render(context, area, style)
    }
}

/// Render the given vote protocol as a PDF document
///
/// Writes the output into the given `io::Write` implementor `w`.
pub(crate) fn generate_pdf(
    db: Arc<Db>,
    protocol: Vec<ProtocolEntry>,
    w: impl io::Write,
) -> Result<()> {
    // including the OpenTalk font in the binary
    let font_data = include_bytes!("font/OpenTalk-500.ttf");

    let regular_font = FontData::new(font_data.to_vec(), None)?;

    let font_family = FontFamily {
        regular: regular_font.clone(),
        bold: regular_font.clone(),
        italic: regular_font.clone(),
        bold_italic: regular_font,
    };

    let mut doc = Document::new(font_family);

    doc.set_title("Vote Protocol");

    let mut decorator = SimplePageDecorator::new();

    decorator.set_margins(12);

    doc.set_page_decorator(decorator);

    doc.set_font_size(10);

    let mut user_cache = UserCache::default();

    // used to collect a vote summary across multiple protocol entries
    let mut summary_table = TableLayout::new(vec![3, 7]);

    let mut final_result = None;

    let mut votes = Vec::new();

    // collect information from all entries
    for entry in protocol {
        match entry.event {
            VoteEvent::Start(start) => {
                summarize_start_entry(
                    db.clone(),
                    &mut user_cache,
                    start,
                    &mut summary_table,
                    entry.timestamp,
                )?;
            }
            VoteEvent::Vote(vote) => votes.push((vote, entry.timestamp)),
            VoteEvent::Stop(stop_kind) => {
                summarize_stop_entry(db.clone(), &mut summary_table, stop_kind, entry.timestamp)?
            }
            VoteEvent::FinalResults(results) => {
                final_result = Some(get_result_elements(results)?);
            }
            VoteEvent::Cancel(cancel) => summarize_cancel_entry(
                db.clone(),
                &mut user_cache,
                &mut summary_table,
                cancel,
                entry.timestamp,
            )?,
        }
    }

    let title = Paragraph::default().styled_string(
        "OpenTalk-Abstimmungsprotokoll",
        Style::default().with_font_size(18),
    );
    doc.push(title);
    doc.push(Break::new(3));

    doc.push(summary_table);

    if let Some(final_results) = final_result {
        doc.push(Break::new(1));

        push_elements(&mut doc, final_results);
    }

    if !votes.is_empty() {
        doc.push(Break::new(1));
        doc.push(Paragraph::new("Erfasste Stimmabgaben:"));

        let votes_table = render_votes(db, &mut user_cache, votes)?;

        doc.push(votes_table);
    }

    doc.render(w)?;

    Ok(())
}

fn summarize_start_entry(
    db: Arc<Db>,
    user_cache: &mut UserCache,
    start: Start,
    table: &mut TableLayout,
    _timestamp: DateTime<Utc>,
) -> Result<()> {
    let Start {
        issuer,
        parameters:
            Parameters {
                initiator_id: _,
                legal_vote_id,
                start_time,
                max_votes,
                inner:
                    UserParameters {
                        name,
                        subtitle,
                        topic,
                        allowed_participants: _,
                        enable_abstain,
                        auto_stop,
                        hidden,
                        duration,
                        create_pdf: _,
                    },
            },
    } = start;

    let issuer_name = user_cache.get_user_name(&mut db.get_conn()?, issuer)?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Titel:"));
    row.push_element(Paragraph::new(name));
    row.push()?;

    if let Some(subtitle) = subtitle {
        let mut row = table.row();
        row.push_element(Paragraph::new("Untertitel:"));
        row.push_element(Paragraph::new(subtitle));
        row.push()?;
    }

    if let Some(topic) = topic {
        let mut row = table.row();
        row.push_element(Paragraph::new("Thema:"));
        row.push_element(Paragraph::new(topic));
        row.push()?;
    }

    let mut row = table.row();
    row.push_element(Break::new(1));
    row.push_element(Break::new(1));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Abstimmungsleiter:"));
    row.push_element(Paragraph::new(issuer_name));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Abstimmungs-ID:"));
    row.push_element(Paragraph::new(legal_vote_id.to_string()));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Beginn: "));
    row.push_element(Paragraph::new(
        start_time.format("%d.%m.%Y %H:%M:%S %Z").to_string(),
    ));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Anzahl Teilnehmer:"));
    row.push_element(Paragraph::new(max_votes.to_string()));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Festgelegte Dauer:"));

    if let Some(duration) = duration {
        row.push_element(Paragraph::new(format!("{} seconds", duration)));
    } else {
        row.push_element(Paragraph::new("Nicht festgelegt"));
    }
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Versteckte Abstimmung:"));
    row.push_element(Paragraph::new(bool_to_string(hidden)));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Enthaltung erlaubt:"));
    row.push_element(Paragraph::new(bool_to_string(enable_abstain)));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Automatisches Ende:"));
    row.push_element(Paragraph::new(bool_to_string(auto_stop)));
    row.push()?;

    Ok(())
}

fn render_votes(
    db: Arc<Db>,
    user_cache: &mut UserCache,
    votes: Vec<(Vote, DateTime<Utc>)>,
) -> Result<TableLayout> {
    let mut votes_table = TableLayout::new(vec![5, 2, 5]);

    let mut conn = db.get_conn()?;

    for (vote, ts) in votes {
        let mut row = votes_table.row();

        let identifier = if let Some(user_info) = vote.user_info {
            let user_name = user_cache.get_user_name(&mut conn, user_info.issuer)?;

            // workaround for genpdf rendering
            break_string(user_name, 36)
        } else {
            "anonym".into()
        };

        row.push_element(Paragraph::new(identifier));

        let vote_string = match vote.option {
            VoteOption::Yes => "ja",
            VoteOption::No => "nein",
            VoteOption::Abstain => "enthalten",
        };

        row.push_element(Paragraph::new(vote_string));

        row.push_element(Paragraph::new(
            ts.format("%d.%m.%Y %H:%M:%S %Z").to_string(),
        ));

        row.push()?;
    }

    Ok(votes_table)
}

fn get_result_elements(results: FinalResults) -> Result<Vec<BoxedElement>> {
    let mut elements = vec![];

    let mut table = TableLayout::new(vec![2, 8]);

    elements.push(BoxedElement::new(Paragraph::new("Ergebnisse:")));

    match results {
        FinalResults::Valid(votes) => {
            let Votes { yes, no, abstain } = votes;

            let mut row = table.row();
            row.push_element(Paragraph::new("Ja:"));
            row.push_element(Paragraph::new(yes.to_string()));
            row.push()?;

            let mut row = table.row();
            row.push_element(Paragraph::new("Nein:"));
            row.push_element(Paragraph::new(no.to_string()));
            row.push()?;

            if let Some(abstain) = abstain {
                let mut row = table.row();
                row.push_element(Paragraph::new("Enthalten:"));
                row.push_element(Paragraph::new(abstain.to_string()));
                row.push()?;
            }
        }
        FinalResults::Invalid(invalid) => {
            let mut row = table.row();
            row.push_element(Paragraph::new("Abstimmung ungültig -"));

            let invalid_string = match invalid {
                Invalid::AbstainDisabled => "Abstimmung enthält unerlaubte Enthaltung",
                Invalid::VoteCountInconsistent => {
                    "Erfasste Stimmanzahl entspricht nicht dem Protokoll"
                }
            };

            row.push_element(Paragraph::new(invalid_string));
            row.push()?;
        }
    }

    elements.push(BoxedElement::new(table));

    Ok(elements)
}

fn summarize_stop_entry(
    db: Arc<Db>,
    table: &mut TableLayout,
    stop: StopKind,
    timestamp: DateTime<Utc>,
) -> Result<()> {
    let mut row = table.row();
    row.push_element(Paragraph::new("Ende: "));
    row.push_element(Paragraph::new(
        timestamp.format("%d.%m.%Y %H:%M:%S %Z").to_string(),
    ));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Abstimmung beendet durch:"));

    match stop {
        StopKind::ByUser(user) => {
            let mut conn = db.get_conn()?;

            let user = User::get(&mut conn, user)?;

            let user_name = format!("{} {}", user.firstname, user.lastname);

            row.push_element(Paragraph::new(user_name));
        }
        StopKind::Auto => {
            row.push_element(Paragraph::new("Alle haben abgestimmt"));
        }
        StopKind::Expired => {
            row.push_element(Paragraph::new("Abgelaufen"));
        }
    }

    row.push()?;

    Ok(())
}

fn summarize_cancel_entry(
    db: Arc<Db>,
    user_cache: &mut UserCache,
    table: &mut TableLayout,
    cancel: Cancel,
    timestamp: DateTime<Utc>,
) -> Result<()> {
    let mut row = table.row();
    row.push_element(Paragraph::new("Ende: "));
    row.push_element(Paragraph::new(
        timestamp.format("%d.%m.%Y %H:%M:%S %Z").to_string(),
    ));
    row.push()?;

    let mut row = table.row();
    row.push_element(Paragraph::new("Abstimmung beendet durch:"));

    let reason = match cancel.reason {
        CancelReason::RoomDestroyed => "Raum geschlossen".into(),
        CancelReason::InitiatorLeft => "Abstimmungsleiter hat den Raum verlassen".into(),
        CancelReason::Custom(custom) => {
            let issuer_name = user_cache.get_user_name(&mut db.get_conn()?, cancel.issuer)?;

            format!("{issuer_name}: {custom}")
        }
    };

    row.push_element(Paragraph::new(reason));

    row.push()?;

    Ok(())
}

fn push_elements(doc: &mut Document, elements: Vec<BoxedElement>) {
    for element in elements {
        doc.push(element)
    }
}

fn bool_to_string(value: bool) -> &'static str {
    if value {
        "Aktiviert"
    } else {
        "Deaktiviert"
    }
}

/// A cache to prevent us from looking up the same user multiple times in the database
#[derive(Default)]
struct UserCache {
    users: HashMap<UserId, String>,
}

impl UserCache {
    fn get_user_name(&mut self, db_conn: &mut DbConnection, user_id: UserId) -> Result<String> {
        if let Some(user_name) = self.users.get(&user_id) {
            return Ok(user_name.clone());
        }

        let user = User::get(db_conn, user_id)?;

        let user_name = format!("{} {}", user.firstname, user.lastname);

        self.users.insert(user_id, user_name.clone());

        Ok(user_name)
    }
}

/// Adds whitespaces to continuos strings (genpdf workaround)
///
/// Note:
/// Genpdf won't render strings if they go beyond their render area. If a string contains whitespaces and goes beyond
/// the render area, the string gets split at the whitespace and continues in a new line.
///
/// Adding spaces after a configured length will prevent genpdf from discarding render content.
/// This is a hacky workaround and should be replaced asap.
fn break_string(s: String, max_length: usize) -> String {
    s.split_whitespace()
        .map(|s| {
            if s.chars().count() <= max_length {
                s.into()
            } else {
                s.chars()
                    .chunks(max_length)
                    .into_iter()
                    .map(|chunk| chunk.collect::<String>())
                    .join(" ")
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

#[cfg(test)]
mod test {
    use controller::prelude::chrono::{TimeZone, Utc};
    use controller::prelude::uuid::Uuid;
    use controller_shared::ParticipantId;
    use db_storage::legal_votes::types::protocol::v1::UserInfo;
    use db_storage::legal_votes::LegalVoteId;
    use db_storage::users::User;
    use std::fs;
    use test_util::TestContext;

    use super::*;

    #[test]
    fn break_name() {
        assert_eq!("Fi rs t La st", break_string("First Last".to_string(), 2));
    }

    #[test]
    fn break_longer_name() {
        assert_eq!(
            "ItsAveryLo ngNameThat WouldFitNo Column Lastname",
            break_string(
                "ItsAveryLongNameThatWouldFitNoColumn Lastname".to_string(),
                10
            )
        );
    }

    #[test]
    fn break_numerical() {
        assert_eq!(
            "123456 123456 123456 123456 123456",
            break_string("123456123456123456 123456123456".to_string(), 6)
        );
    }

    #[test]
    fn break_non_ascii() {
        assert_eq!(
            "エンコ ード大 好き",
            break_string("エンコード大好き".to_string(), 3)
        );
    }

    // A little test to manually generate a PDF as a local file at `/tmp/protocol.pdf`
    //
    // To create and open the example PDF in firefox run:
    // `cargo test --package k3k-legal-vote --lib -- pdf::test::generate_pdf_file --exact --ignored --nocapture && firefox /tmp/protocol.pdf`
    #[actix_rt::test]
    #[ignore]
    async fn generate_pdf_file() {
        let test_ctx = TestContext::new().await;

        let db = test_ctx.db_ctx.db.clone();

        let mut users = vec![];

        for i in 0..15 {
            let user: User = test_ctx.db_ctx.create_test_user(i, vec![]).unwrap();
            users.push(user);
        }

        let start = Start {
            issuer: users.first().unwrap().id,
            parameters: Parameters {
                initiator_id: ParticipantId::nil(),
                legal_vote_id: LegalVoteId::from(Uuid::nil()),
                start_time: Utc.ymd(1970, 1, 1).and_hms(0, 0, 0),
                max_votes: 3,
                inner: UserParameters {
                    name: "Weather Vote".into(),
                    subtitle: Some("Another one of these weather votes".into()),
                    topic: Some("Is the weather good today?".into()),
                    allowed_participants: users.iter().map(|_| ParticipantId::nil()).collect(),
                    enable_abstain: false,
                    auto_stop: false,
                    hidden: false,
                    duration: None,
                    create_pdf: true,
                },
            },
        };

        let results = FinalResults::Valid(Votes {
            yes: 0,
            no: 15,
            abstain: None,
        });

        let mut votes = users
            .iter()
            .map(|user| {
                ProtocolEntry::new(VoteEvent::Vote(Vote {
                    user_info: Some(UserInfo {
                        issuer: user.id,
                        participant_id: ParticipantId::nil(),
                    }),
                    option: VoteOption::No,
                }))
            })
            .collect();

        let mut protocol = vec![
            ProtocolEntry::new(VoteEvent::Start(start)),
            ProtocolEntry::new(VoteEvent::FinalResults(results)),
            ProtocolEntry::new(VoteEvent::Stop(StopKind::Auto)),
        ];

        protocol.append(&mut votes);

        let buffer = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open("/tmp/protocol.pdf")
            .expect("failed to create/open /tmp/protocol.pdf");

        generate_pdf(db, protocol, buffer).unwrap();
    }
}
