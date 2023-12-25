#!/bin/bash
set -e -o pipefail
set -x

if [ $# -eq 0 ]; then
	echo "Usage: $0 [all|dev|prod|server(s)]"
	exit 1
fi

if [ "$1" = "all" ]; then
	SERVERS=("miskatonic" "hamr" "poseidon" "kalevala" "atwood")
elif [ "$1" = "dev" ]; then
	SERVERS=("miskatonic" "atwood" "poseidon")
elif [ "$1" = "prod" ]; then
	SERVERS=("kalevala" "hamr")
else
	SERVERS=("$@")
fi

echo "Syncing with " "${SERVERS[@]}"

for server in "${SERVERS[@]}"; do
	rsync -urltv --exclude .env --exclude 'target' --exclude 'data/settings' --exclude cracktunes.toml -e ssh /home/lothrop/dev/cracktunes "${server}:${HOME}/src/"
done
