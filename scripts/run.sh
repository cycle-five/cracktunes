#!/bin/sh
unzip deploy.zip
# shellcheck source=/dev/null
. "$HOME/cracktunes.env"
tmux new-session -s cracktunes -d
tmux send-keys -t grafana-agent "$HOME/cracktunes" Enter
