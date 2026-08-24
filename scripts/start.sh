#!/bin/sh
#
# Container entrypoint.
#
# /app/.env is sourced only if it is actually a readable file. It used to be
# `. /app/.env && exec app`, which made the env file mandatory: with the stack
# driven through a remote docker context, compose resolves `./.env` to an
# absolute path on the DOCKER HOST, docker creates an empty directory there when
# it does not exist, and sourcing a directory fails — so the `&&` short-circuited
# and the bot never started. Config now arrives through compose's `env_file:`,
# which injects variables directly and needs nothing on the host filesystem.
#
# exec so the bot is PID 1 and receives SIGTERM from `docker stop` directly,
# rather than sh swallowing it and the container waiting out the kill timeout.
set -e

if [ -f /app/.env ] && [ -r /app/.env ]; then
  . /app/.env
fi

RUST_BACKTRACE=full exec /app/app
