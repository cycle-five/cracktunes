#!/bin/bash
SERVERS=("olympus:~/" "kalevala:~/dev/" "poseidon:~/dev/")
for server in ${SERVERS[*]}; do
	rsync -urltv --exclude 'target' -e ssh /home/lothrop/dev/cracktunes "${server}"
done
