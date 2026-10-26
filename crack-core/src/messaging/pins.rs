//! Pins that convert neutral messages to Discord JSON. They need crack-core's
//! `messaging::discord`, `interface` and `buttons`, so they stay here while the
//! code they pin lives in crack-common.

mod render_pins {
    use crate::messaging::discord::{ToDiscord, ToPoiseReply};
    use crate::messaging::message::CrackedMessage;
    use crate::messaging::render::*;
    use crack_model::Card;

    fn cx() -> RenderCx {
        RenderCx {
            now_unix: 0,
            embed_links: true,
        }
    }

    /// An edit replaces everything: a message edited to one without an embed
    /// must lose its old embed and content.
    #[test]
    fn an_edit_without_an_embed_clears_the_old_one() {
        let json = serde_json::to_value(Rendered::default().to_edit()).expect("serializes");
        assert_eq!(json["embeds"], serde_json::json!([]));
        assert_eq!(json["content"], serde_json::json!(""));
    }

    #[test]
    fn nothing_rendered_may_ping_by_default() {
        let r = render(&CrackedMessage::Other("@everyone".into()), &cx());
        assert_eq!(r.mentions, Mentions::None);
        let am = serde_json::to_value(r.allowed_mentions()).unwrap();
        assert_eq!(am["parse"], serde_json::json!([]));
    }

    /// A follow-up to a press: private when asked, pings nobody, carries the embed.
    #[test]
    fn a_followup_is_private_when_asked_and_pings_nobody() {
        let r = render(&CrackedMessage::Other("@everyone".into()), &cx());
        let private = serde_json::to_value(r.to_followup(true)).unwrap();
        assert_eq!(private["flags"].as_u64(), Some(64));
        assert_eq!(private["allowed_mentions"]["parse"], serde_json::json!([]));
        assert_eq!(private["embeds"].as_array().map(Vec::len), Some(1));
        let public = serde_json::to_value(r.to_followup(false)).unwrap();
        assert_eq!(public["flags"].as_u64().unwrap_or(0) & 64, 0);
    }

    /// A page flip carries the new embed and the nav buttons, and pings nobody.
    #[test]
    fn an_interaction_update_carries_the_embed_and_the_buttons() {
        let r = Rendered::card(Card {
            title: Some("Page 2".into()),
            ..Card::default()
        })
        .with_rows(crate::messaging::interface::create_nav_btns(1, 3));
        let v = serde_json::to_value(r.to_interaction_message()).unwrap();
        assert_eq!(v["embeds"][0]["title"], "Page 2");
        assert_eq!(v["components"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["allowed_mentions"]["parse"], serde_json::json!([]));
    }

    fn embed_titles(v: &serde_json::Value) -> Vec<String> {
        v["embeds"]
            .as_array()
            .expect("embeds")
            .iter()
            .map(|e| e["title"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    /// Search results are one message with an embed per hit: every send and
    /// edit carries all of them, in order.
    #[test]
    fn extra_embeds_follow_the_embed_everywhere() {
        let titled = |t: &str| Card {
            title: Some(t.into()),
            ..Card::default()
        };
        let r = Rendered::card(titled("a")).with_cards(vec![titled("b"), titled("c")]);
        let abc = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];

        let sent = serde_json::to_value(r.to_message()).unwrap();
        assert_eq!(embed_titles(&sent), abc);
        let edited = serde_json::to_value(r.to_edit()).unwrap();
        assert_eq!(embed_titles(&edited), abc);
        let reply = r
            .to_reply(false)
            .to_slash_initial_response(serenity::all::CreateInteractionResponseMessage::new());
        assert_eq!(embed_titles(&serde_json::to_value(reply).unwrap()), abc);
        let reply_edit = r
            .to_reply_edit()
            .0
            .to_prefix_edit(serenity::all::EditMessage::new());
        assert_eq!(
            embed_titles(&serde_json::to_value(reply_edit).unwrap()),
            abc
        );
    }

    /// The flag `to_reply` is given is the one Discord gets, set either way:
    /// poise fills in a command's `ephemeral` attribute only when a reply
    /// leaves it unset, so `false` makes even an `ephemeral` command public.
    #[test]
    fn a_replys_ephemeral_flag_reaches_the_wire() {
        let flags = |ephemeral: bool| {
            let r = Rendered::text("x")
                .to_reply(ephemeral)
                .to_slash_initial_response(serenity::all::CreateInteractionResponseMessage::new());
            serde_json::to_value(r).unwrap()["flags"].as_u64()
        };
        assert_eq!(flags(true), Some(64));
        assert_eq!(flags(false).unwrap_or(0) & 64, 0);
    }

    #[test]
    fn errors_are_red() {
        let r = render(&CrackedMessage::Error, &cx());
        let v = serde_json::to_value(crate::messaging::discord::embed(&r.cards[0])).unwrap();
        assert_eq!(v["color"], serde_json::json!(serenity::all::Colour::RED.0));
    }
}

mod card_pins {
    use crate::messaging::cards::*;
    use crate::messaging::discord::ToDiscord;
    use crate::messaging::format::EMBED_TITLE_MAX;
    use crate::messaging::format::{Progress, TrackLabel};
    use crate::messaging::messages::STATUS_FINISHED_TITLE;
    use crate::messaging::render::{RenderCx, Rendered};
    use crack_types::discord::ToRef;
    use serenity::all::UserId;
    use std::time::Duration;

    fn card(url: Option<&str>, thumb: Option<&str>) -> NowPlayingCard {
        NowPlayingCard {
            label: TrackLabel {
                title: Some("OH SHIT I'M FEELING IT".into()),
                url: url.map(str::to_owned),
                duration: Some(Duration::from_secs(273)),
            },
            thumbnail: thumb.map(str::to_owned),
            requester: Some(UserId::new(42).to_ref()),
            progress: Progress::Playing {
                position: Duration::from_secs(73),
                duration: Some(Duration::from_secs(273)),
            },
            rows: Vec::new(),
        }
    }

    fn queued_card(duration: Option<Duration>, wait: Option<Duration>) -> QueuedCard {
        QueuedCard {
            author: "📃 Added to queue!",
            label: TrackLabel {
                title: Some("OH SHIT I'M FEELING IT".into()),
                url: Some("https://www.youtube.com/watch?v=x".into()),
                duration,
            },
            thumbnail: Some("https://i.ytimg.com/a.jpg".into()),
            wait,
        }
    }

    fn v(r: &Rendered) -> serde_json::Value {
        serde_json::to_value(crate::messaging::discord::first_embed(r).unwrap()).unwrap()
    }

    #[test]
    fn the_now_playing_card_shows_a_live_end_time() {
        let r = now_playing(
            &card(
                Some("https://www.youtube.com/watch?v=x"),
                Some("https://i.ytimg.com/a.jpg"),
            ),
            &RenderCx {
                now_unix: 1_000_000,
                embed_links: true,
            },
        );
        let e = v(&r);
        assert_eq!(e["title"], "OH SHIT I'M FEELING IT");
        assert_eq!(e["url"], "https://www.youtube.com/watch?v=x");
        assert_eq!(e["fields"][0]["value"], ">>> 4:33 · ends <t:1000200:R>");
        assert_eq!(e["fields"][1]["value"], ">>> <@42>");
        assert_eq!(e["thumbnail"]["url"], "https://i.ytimg.com/a.jpg");
        assert_eq!(e["footer"]["text"], "Streaming via youtube.com");
    }

    /// The members-only link on TuneTitan (2026-10-06): no title, no URL,
    /// no thumbnail. No `RelativeUrlWithoutBase`, no "Streaming via unknown".
    #[test]
    fn a_track_with_nothing_known_renders_cleanly() {
        let mut c = card(None, Some(""));
        c.label = TrackLabel::default();
        c.progress = Progress::Playing {
            position: Duration::ZERO,
            duration: None,
        };
        let e = v(&now_playing(
            &c,
            &RenderCx {
                now_unix: 50,
                embed_links: true,
            },
        ));
        assert_eq!(e["title"], "(untitled)");
        assert!(e.get("url").is_none() || e["url"].is_null());
        assert!(e.get("thumbnail").is_none() || e["thumbnail"].is_null());
        assert!(e.get("footer").is_none() || e["footer"].is_null());
        assert_eq!(e["fields"][0]["value"], ">>> Started <t:50:R>");
    }

    /// A link that is not http(s) is not a link: no url, thumbnail or footer.
    #[test]
    fn a_non_http_url_is_not_rendered() {
        let e = v(&now_playing(
            &card(Some("javascript:alert(1)"), Some("file:///etc/passwd")),
            &RenderCx::default(),
        ));
        for key in ["url", "thumbnail", "footer"] {
            assert!(e.get(key).is_none() || e[key].is_null(), "{key}: {e}");
        }
    }

    #[test]
    fn a_paused_card_says_where_it_paused() {
        let mut c = card(None, None);
        c.progress = Progress::Paused {
            position: Some(Duration::from_secs(72)),
        };
        assert_eq!(
            v(&now_playing(
                &c,
                &RenderCx {
                    now_unix: 0,
                    embed_links: true
                }
            ))["fields"][0]["value"],
            ">>> Paused at 1:12"
        );
    }

    /// Ruling R9: a track on repeat never reaches "ends …", so the card says
    /// it is on repeat instead of an end time that goes stale.
    #[test]
    fn a_card_on_repeat_has_no_end_time() {
        let mut c = card(None, None);
        c.progress = Progress::Repeating {
            duration: Some(Duration::from_secs(273)),
        };
        let cx = RenderCx {
            now_unix: 1_000_000,
            embed_links: true,
        };
        assert_eq!(
            v(&now_playing(&c, &cx))["fields"][0]["value"],
            ">>> 4:33 · on repeat"
        );
    }

    /// Ruling R2: escaping after the cap can push a title past Discord's limit.
    #[test]
    fn an_escaped_title_still_fits_the_embed_title() {
        let mut c = card(None, None);
        c.label.title = Some("*".repeat(300));
        let e = v(&now_playing(&c, &RenderCx::default()));
        assert!(e["title"].as_str().unwrap().chars().count() <= EMBED_TITLE_MAX);
    }

    #[test]
    fn the_finished_card_is_the_status_embed() {
        let e = v(&finished());
        assert_eq!(e["title"], STATUS_FINISHED_TITLE);
    }

    /// "Added to queue! Track duration: 00:00" was computed from a length
    /// nobody knew; now an unknown length and wait say nothing at all.
    #[test]
    fn a_queued_track_of_unknown_length_has_no_footer() {
        let e = v(&queued(&queued_card(None, None), &RenderCx::default()));
        assert!(e.get("footer").is_none() || e["footer"].is_null(), "{e}");
        assert_eq!(e["author"]["name"], "📃 Added to queue!");
        assert_eq!(e["title"], "OH SHIT I'M FEELING IT");
        assert_eq!(e["url"], "https://www.youtube.com/watch?v=x");
        assert_eq!(e["thumbnail"]["url"], "https://i.ytimg.com/a.jpg");
    }

    #[test]
    fn a_queued_track_says_its_length_and_its_wait() {
        let e = v(&queued(
            &queued_card(
                Some(Duration::from_secs(273)),
                Some(Duration::from_secs(48)),
            ),
            &RenderCx::default(),
        ));
        assert_eq!(
            e["footer"]["text"],
            "Track duration: 4:33\nEstimated time until play: 0:48"
        );
    }

    /// Each line stands alone: a known length with an unknown wait (a live
    /// stream ahead of it) still says the length.
    #[test]
    fn a_queued_track_with_an_unknown_wait_says_only_its_length() {
        let e = v(&queued(
            &queued_card(Some(Duration::from_secs(273)), None),
            &RenderCx::default(),
        ));
        assert_eq!(e["footer"]["text"], "Track duration: 4:33");
    }

    #[test]
    fn an_untitled_queued_track_is_named_untitled() {
        let mut c = queued_card(None, None);
        c.label.title = Some("  ".into());
        assert_eq!(v(&queued(&c, &RenderCx::default()))["title"], "(untitled)");
    }

    /// A link or thumbnail that is not http(s) is left out, not sent broken.
    #[test]
    fn a_queued_track_without_a_web_link_has_no_url_or_thumbnail() {
        let mut c = queued_card(None, None);
        c.label.url = Some("javascript:alert(1)".into());
        c.thumbnail = Some(String::new());
        let e = v(&queued(&c, &RenderCx::default()));
        for key in ["url", "thumbnail"] {
            assert!(e.get(key).is_none() || e[key].is_null(), "{key}: {e}");
        }
    }

    /// Ruling R2, for the queued card too.
    #[test]
    fn an_escaped_queued_title_still_fits_the_embed_title() {
        let mut c = queued_card(None, None);
        c.label.title = Some("*".repeat(300));
        let e = v(&queued(&c, &RenderCx::default()));
        assert!(e["title"].as_str().unwrap().chars().count() <= EMBED_TITLE_MAX);
    }

    #[test]
    fn a_card_with_controls_carries_one_row_and_one_without_carries_none() {
        use crate::messaging::buttons::Controls;
        let mut c = card(None, None);
        assert!(now_playing(&c, &RenderCx::default()).rows.is_empty());
        c.rows = Controls {
            guild: serenity::all::GuildId::new(1).to_ref(),
            track: uuid::Uuid::nil(),
            paused: false,
            looping: false,
        }
        .rows();
        let r = now_playing(&c, &RenderCx::default());
        assert_eq!(r.rows.len(), 1);
        let row =
            serde_json::to_value(&crate::messaging::discord::action_rows(&r.rows)[0]).unwrap();
        assert_eq!(row["components"][0]["custom_id"], "np:pause:1");
    }

    /// The ids name a Discord server: a guild that is no snowflake has none.
    #[test]
    fn a_non_snowflake_guild_gets_no_buttons() {
        use crate::messaging::buttons::Controls;
        let mut c = card(None, None);
        c.rows = Controls {
            guild: crack_model::CommunityRef::new(crack_model::Platform::Discord, "0").unwrap(),
            track: uuid::Uuid::nil(),
            paused: false,
            looping: false,
        }
        .rows();
        let with = now_playing(&c, &RenderCx::default());
        assert!(with.rows.is_empty());
        c.rows = Vec::new();
        let without = now_playing(&c, &RenderCx::default());
        assert_eq!(with.cards, without.cards);
        assert_eq!(with.rows, without.rows);
    }

    /// "None on Finished": the edit to Finished clears the row.
    #[test]
    fn the_finished_card_has_no_buttons() {
        assert!(finished().rows.is_empty());
        let edit = serde_json::to_value(finished().to_edit()).unwrap();
        assert_eq!(edit["components"].as_array().map(Vec::len), Some(0));
    }

    /// The Discord bytes of each music message, captured before the cards
    /// were built neutral. Only the payloads' construction may change.
    #[test]
    fn music_cards_render_as_before() {
        use crate::messaging::buttons::Controls;
        use crate::messaging::failure::{FailReason, Failure};
        use crate::messaging::message::CrackedMessage;
        use crate::messaging::render::render;

        fn bytes(r: &Rendered) -> (String, String, String) {
            (
                serde_json::to_string(&crate::messaging::discord::first_embed(r)).unwrap(),
                serde_json::to_string(&crate::messaging::discord::action_rows(&r.rows)).unwrap(),
                serde_json::to_string(&crate::messaging::discord::extra_embeds(r)).unwrap(),
            )
        }
        fn pin(r: &Rendered, embed: &str, components: &str) {
            assert_eq!(
                bytes(r),
                (embed.to_owned(), components.to_owned(), "[]".to_owned())
            );
        }
        let cx = RenderCx {
            now_unix: 1_000_000,
            embed_links: true,
        };

        let mut playing = card(
            Some("https://www.youtube.com/watch?v=x"),
            Some("https://i.ytimg.com/a.jpg"),
        );
        playing.rows = Controls {
            guild: serenity::all::GuildId::new(7).to_ref(),
            track: uuid::Uuid::nil(),
            paused: false,
            looping: true,
        }
        .rows();
        pin(&now_playing(&playing, &cx), "{\"title\":\"OH SHIT I'M FEELING IT\",\"type\":\"rich\",\"description\":\"[vote](https://top.gg/bot/1115229568006103122/vote) • [invite](https://discord.com/oauth2/authorize?client_id=1115229568006103122&permissions=551940115520&scope=bot+applications.commands)\",\"url\":\"https://www.youtube.com/watch?v=x\",\"footer\":{\"text\":\"Streaming via youtube.com\",\"icon_url\":\"https://www.google.com/s2/favicons?domain=youtube.com\"},\"thumbnail\":{\"url\":\"https://i.ytimg.com/a.jpg\",\"description\":null},\"author\":{\"name\":\"🔊 Now playing\",\"url\":null,\"icon_url\":null},\"fields\":[{\"name\":\"Progress\",\"value\":\">>> 4:33 · ends <t:1000200:R>\",\"inline\":true},{\"name\":\"Requested by\",\"value\":\">>> <@42>\",\"inline\":true}]}",
            "[{\"type\":1,\"components\":[{\"style\":2,\"type\":2,\"custom_id\":\"np:pause:7\",\"emoji\":{\"name\":\"⏸\u{fe0f}\"},\"disabled\":false},{\"style\":2,\"type\":2,\"custom_id\":\"np:skip:7:00000000-0000-0000-0000-000000000000\",\"emoji\":{\"name\":\"⏭\u{fe0f}\"},\"disabled\":false},{\"style\":3,\"type\":2,\"custom_id\":\"np:repeat-off:7\",\"emoji\":{\"name\":\"🔁\"},\"disabled\":false},{\"style\":2,\"type\":2,\"custom_id\":\"np:shuffle:7\",\"emoji\":{\"name\":\"🔀\"},\"disabled\":false}]}]");

        pin(
            &queued(
                &queued_card(
                    Some(Duration::from_secs(273)),
                    Some(Duration::from_secs(48)),
                ),
                &cx,
            ),
            "{\"title\":\"OH SHIT I'M FEELING IT\",\"type\":\"rich\",\"url\":\"https://www.youtube.com/watch?v=x\",\"footer\":{\"text\":\"Track duration: 4:33\\nEstimated time until play: 0:48\",\"icon_url\":null},\"thumbnail\":{\"url\":\"https://i.ytimg.com/a.jpg\",\"description\":null},\"author\":{\"name\":\"📃 Added to queue!\",\"url\":null,\"icon_url\":null}}",
            "[]",
        );

        let line = EchoLine {
            echo: Echo::Skipped {
                title: Some("A*B".into()),
            },
            user: UserId::new(42).to_ref(),
            via: Via::Dashboard,
        };
        pin(&echo(&line), "{\"type\":\"rich\",\"description\":\"⏭ Skipped **A\\\\*B** from the dashboard — <@42>\"}",
            "[]");
        pin(&finished(), "{\"title\":\"⏹\u{fe0f} Finished\",\"type\":\"rich\",\"description\":\"Nothing is playing. Use /play to start again.\"}",
            "[]");

        let failed = CrackedMessage::TrackFailed {
            listed: vec![
                Failure {
                    title: Some("one".into()),
                    reason: FailReason::Open,
                },
                Failure {
                    title: None,
                    reason: FailReason::Seek,
                },
            ],
            more: 3,
        };
        pin(&render(&failed, &cx), "{\"type\":\"rich\",\"description\":\"⚠\u{fe0f} Couldn't play 5 tracks:\\n• **one**: couldn't open the stream\\n• **(untitled)**: couldn't seek in it\\n+3 more\"}",
            "[]");

        pin(&render(&CrackedMessage::Error, &cx), "{\"type\":\"rich\",\"description\":\"Fatality! Something went wrong ☹\u{fe0f}\",\"color\":15158332}",
            "[]");
    }
}
