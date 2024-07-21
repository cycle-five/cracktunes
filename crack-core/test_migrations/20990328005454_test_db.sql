-- -- Add migration script here
INSERT INTO "user" (
        id,
        username,
        discriminator,
        avatar_url,
        bot,
        created_at,
        updated_at,
        last_seen
    )
VALUES (
        1,
        '🔧 Test',
        1234,
        'https://example.com/avatar.jpg',
        false,
        NOW(),
        NOW(),
        NOW()
    );
INSERT INTO guild (id, "name", created_at, updated_at)
VALUES (1, '🔧 Test', NOW(), NOW());
INSERT INTO guild_settings (
        guild_id,
        guild_name,
        prefix,
        premium,
        autopause,
        allow_all_domains,
        allowed_domains,
        banned_domains,
        ignored_channels,
        old_volume,
        volume,
        self_deafen,
        timeout_seconds,
        additional_prefixes
    )
VALUES (
        1,
        '🔧 Test',
        'r!',
        false,
        false,
        true,
        '{}',
        '{}',
        '{}',
        1.0,
        1.0,
        true,
        360,
        '{}'
    );
INSERT INTO metadata (
        track,
        artist,
        album,
        date,
        channels,
        channel,
        start_time,
        duration,
        sample_rate,
        source_url,
        title,
        thumbnail
    )
VALUES (
        '🔧 Test',
        '🔧 Test',
        '🔧 Test',
        '2023-11-13',
        2,
        '🔧 Test',
        0,
        0,
        0,
        'https://example.com',
        '🔧 Test',
        'https://example.com/thumbnail.jpg'
    );