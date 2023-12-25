#!/bin/bash
set -e -o pipefail
set -x

cargo build --release
zip deploy.zip -r target/release/cracktunes .sqlx data scripts/run.sh
scp deploy.zip kalevala:~/

ssh kalevala -c '$HOME/run.sh'


