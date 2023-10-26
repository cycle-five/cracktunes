#!/bin/bash
if $# -eq 0; then
	echo "Usage: $0 [all|server(s)]"
	exit 1
fi

if [ $1 = "all" ]; then
	SERVERS=("kalevala" "hamr" "poseidon")
else
	SERVERS=("$*")
fi

SERVERS=("kalevala" "hamr")
for server in "${SERVERS[@]}"; do
	rsync -urltv --exclude .env --exclude 'target' --exclude 'data/settings' --exclude cracktunes.toml -e ssh /home/lothrop/dev/cracktunes "${server}:~/src/"
done
