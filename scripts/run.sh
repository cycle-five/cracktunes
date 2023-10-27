#!/bin/sh
unzip deploy.zip
source $HOME/cracktunes.env
tmux new-session -s cracktunes -d
tmux send-keys -t grafana-agent "$HOME/cracktunes" Enter
