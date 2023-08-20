#!/bin/bash
SERVERS=("kalevala:~/dev/" "karmic:~/dev/")
for server in ${SERVERS[*]}; do
	# rsync -urltv --exclude 'target' --delete -e ssh /home/lothrop/dev/cracktunes "${server}"
	rsync -urltv --exclude 'target' --exclude cracktunes.toml -e ssh /home/lothrop/dev/cracktunes "${server}"
done
