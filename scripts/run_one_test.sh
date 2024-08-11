cargo test \
	--package crack-core \
	--lib \
	--features crack-gpt --features crack-osint --features crack-bf \
	-- db::guild::test::test_update_prefix \
	--exact \
	--show-output

cargo test \
	--package crack-voting \
	--lib \
	-- test::test_authorized \
	--exact \
	--show-output

cargo test \
	--package crack-core \
	--lib \
	--features crack-gpt --features crack-osint --features crack-bf \
	-- sources::youtube::test::test_get_track_source_and_metadata \
	--exact \
	--show-output
