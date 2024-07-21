cargo test \
	--package crack-core \
	--lib \
	--features crack-gpt --features crack-osint --features crack-bf \
	-- db::worker_pool::test::test_workers \
	--exact \
	--show-output
