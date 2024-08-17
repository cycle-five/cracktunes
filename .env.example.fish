#
# [REQUIRED] To authenticate with Discord, you must create a Discord app.
# See more: https://discord.com/developers/applications
set -x DISCORD_TOKEN XXXXXX
set -x DISCORD_APP_ID XXXXXX

#
# [REQUIRED] Postgres database URL for the bot to use.
#
set -x DATABASE_URL postgresql://postgres:mysecretpassword@localhost:5432/postgres
set -x PG_USER postgres
set -x PG_PASSWORD mysecretpassword

#
# [Optional] To support Spotify links, you must create a Spotify app.
# See more: https://developer.spotify.com/dashboard/applications
set -x SPOTIFY_CLIENT_ID XXXXXX
set -x SPOTIFY_CLIENT_SECRET XXXXXX

#
# [Optional] OpenAI API key for the chatgpt feature.
#
set -x OPENAI_API_KEY XXXXXX

#
# [Optional] pgadmin support
#
set -x PGADMIN_MAIL XXXXXX
set -x PGADMIN_PW XXXXXX

#
# [Optional] VirusTotal API key for the url scanning.
#
set -x VIRUSTOTAL_API_KEY XXXXXX

#
# [Optional] top.gg and discordbotlist.com integration.
#
set -x TOPGG_TOKEN XXXXXX
set -x DBL_TOKEN XXXXXX
set -x WEBHOOK_SECRET XXXXXX
