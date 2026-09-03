#!/usr/bin/env bash
#
# cracktunes.sh — THE deploy/inspect front door for the bot's docker stack.
# Every routine operation runs through here; the README and CLAUDE.md refer to
# these subcommands as the canonical vocabulary. Nothing else in scripts/ is a
# supported way to deploy.
#
# Always invokes compose with `-p $STACK --env-file $ENV_FILE` so a partial or
# typo'd command cannot operate on a different stack than intended.
#
# Config via env (sensible defaults):
#   STACK             compose project name         (default: cracktunes)
#   ENV_FILE          env file compose reads       (default: .env)
#   EXPECTED_CONTEXT  docker context to require    (default: pve-staging)
#
# Guards. Each exists because the failure it prevents is silent. How they
# behave depends on what you asked for:
#
#   * State-changing subcommands (up, deploy, restart, down, destroy, migrate)
#     refuse at the FIRST failed guard. These replace a working container, so
#     stopping early is the point.
#   * Read-only subcommands (preflight, ps, logs, status) never refuse. They
#     degrade: preflight runs every guard and reports all of them before
#     exiting nonzero, so you fix the set rather than rediscovering one per
#     run; ps/logs/status warn and carry on, because reading the wrong host or
#     reading without a .env is recoverable by looking at the banner.
#
#   1. DOCKER CONTEXT. `docker compose` targets whatever context is current, and
#      a machine may have several pointing at remote hosts. Deploying to the
#      wrong one does not fail — it builds a complete parallel stack over
#      there, with its own network and volumes, and looks like success.
#
#      The default is `default`, the local docker socket: this compose file is
#      the self-host stack (postgres, pgadmin, the bot), and a clone of this
#      repo has no idea what remote contexts you keep. Deploying anywhere else
#      has to be asked for, which is the guard doing its job:
#          EXPECTED_CONTEXT=my-host DOCKER_CONTEXT=my-host ./scripts/cracktunes.sh up
#
#      Relatedly: the compose file no longer bind-mounts anything from this
#      checkout. It used to mount ./.env and ./cracktunes.toml, which resolve
#      on the DOCKER HOST — so driving a remote context meant the bot read
#      whatever sat at those paths over there, or an empty directory docker
#      helpfully created. Config now arrives via compose's `env_file:`.
#
#   2. DATABASE PASSWORD DIVERGENCE. docker-compose.yml hardcodes
#      `DATABASE_URL=postgresql://postgres:mysecretpassword@...` for the app
#      services while crack-postgres honours ${POSTGRES_PASSWORD}. Set
#      POSTGRES_PASSWORD to anything else and postgres comes up healthy while
#      every app service fails to authenticate. `preflight` compares the two and
#      refuses before that happens.
#
#   3. EXTERNAL VOLUMES. pgdata and crack_data are declared `external: true`, so
#      a cold `up` on a fresh host fails on a missing volume rather than
#      creating one. Checked up front, with the exact command to fix it.
#
#   4. MISSING DISCORD_TOKEN. The bot fail-closes at BOOT, not at deploy: a
#      missing token is not a failed deploy, it is a container that panics and
#      restarts forever while the old one is already gone. Checked before
#      anything is replaced.
#
# Subcommands:
#   preflight        Run every guard and report ALL failures, changing nothing.
#                    Exits nonzero if any failed. Safe anywhere.
#   up               Bring the stack up (compose up -d).
#   deploy [svc]     Pull fresh images and force-recreate. This is the one that
#                    actually ships a new version. Services pin floating tags
#                    and set no pull_policy, so plain `up` will happily keep
#                    running a stale cached image.
#   restart [svc]    Recreate the container WITHOUT pulling. Picks up no new
#                    code — only use it to bounce a process you know is current.
#   stop             Stop the stack, preserving volumes.
#   down             Stop and remove containers, preserving volumes.
#   destroy          compose down -v — DROPS THE DATABASE (guild settings,
#                    playlists, play logs). Requires typing the stack name.
#   ps               compose ps.
#   logs [svc]       Tail logs (default: 50 lines, follow).
#   shell <svc>      Open a shell in a container.
#   psql             Open psql in the postgres container over its unix socket.
#   sql [psql-args]  Run read-only SQL from stdin. Read-only is enforced by the
#                    server, not by trusting the caller's query.
#   migrate          Run sqlx migrations against the stack's database.
#   status           What is running, which image, and the bot's own version.
#   help             Show this help.
#
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$SCRIPT_DIR"

STACK="${STACK:-cracktunes}"
ENV_FILE="${ENV_FILE:-.env}"
EXPECTED_CONTEXT="${EXPECTED_CONTEXT:-default}"
CURRENT_CONTEXT="$(docker context show 2>/dev/null || echo unknown)"
ORIGINAL_ARGS="$*"

# The password baked into docker-compose.yml's DATABASE_URL for the app
# services. Read from the file rather than duplicated here, so this keeps
# working if the compose file is edited.
compose_db_password() {
  grep -oE 'DATABASE_URL=postgresql://[^:]+:[^@]+@' docker-compose.yml \
    | head -1 | sed 's|.*://[^:]*:||; s|@$||'
}

c_r=$'\033[31m'; c_g=$'\033[32m'; c_y=$'\033[33m'; c_b=$'\033[1m'; c_z=$'\033[0m'
ok()   { printf "%s✓%s %s\n" "$c_g" "$c_z" "$*"; }
warn() { printf "%s!%s %s\n" "$c_y" "$c_z" "$*"; }
die()  { printf "%s✗ %s%s\n" "$c_r" "$*" "$c_z" >&2; exit 1; }
hdr()  { printf "\n%s== %s ==%s\n" "$c_b" "$*" "$c_z"; }

# STRICT=1 (the default) is for anything that changes the stack: the first
# failed guard exits. preflight clears it so a single run can report every
# problem instead of handing them back one per invocation.
STRICT=1
PROBLEMS=0
fail() {
  if (( STRICT )); then
    die "$@"
  fi
  printf "%s✗%s %s\n" "$c_r" "$c_z" "$*" >&2
  PROBLEMS=$((PROBLEMS + 1))
}

read_env() {
  # Read one key out of $ENV_FILE without echoing the value. Tolerates the
  # `export KEY=value` form the .env.example ships, and strips surrounding
  # quotes. (docker compose itself also accepts `export`; verified.)
  [[ -f "$ENV_FILE" ]] || return 0
  grep -E "^(export[[:space:]]+)?${1}=" "$ENV_FILE" 2>/dev/null \
    | head -1 | sed 's/^export[[:space:]]*//' | cut -d= -f2- | sed 's/^"//; s/"$//'
}

# ---------------------------------------------------------------- guards ----

require_docker_context() {
  [[ -n "${_CONTEXT_OK:-}" ]] && return 0

  # DOCKER_HOST outranks the context, but `docker context show` keeps reporting
  # the context name regardless — so the comparison below would be checking a
  # value that is not deciding anything. Refuse rather than pretend to verify.
  if [[ -n "${DOCKER_HOST:-}" ]]; then
    die "DOCKER_HOST is set ($DOCKER_HOST), which overrides the docker context.
       This script cannot verify which host it would deploy to.
       Unset it and use DOCKER_CONTEXT=$EXPECTED_CONTEXT instead."
  fi

  if [[ "$CURRENT_CONTEXT" != "$EXPECTED_CONTEXT" ]]; then
    die "docker context is '$CURRENT_CONTEXT', expected '$EXPECTED_CONTEXT'.

       Deploying from the wrong context does NOT fail. It builds a complete
       parallel stack on that host, with its own network and volumes, and
       looks exactly like success — the only tell is that every container
       says 'Creating' rather than 'Recreating'.

       Fix:       DOCKER_CONTEXT=$EXPECTED_CONTEXT $0 $ORIGINAL_ARGS
       Intended?  EXPECTED_CONTEXT=$CURRENT_CONTEXT $0 $ORIGINAL_ARGS"
  fi

  _CONTEXT_OK=1
}

require_env_file() {
  [[ -f "$ENV_FILE" ]] || die "$ENV_FILE not found in $SCRIPT_DIR
       Copy .env.example to .env and fill it in, or set ENV_FILE=<path>."
}

check_db_password() {
  # See guard 2 in the header.
  local baked env_pw
  baked="$(compose_db_password)"
  env_pw="$(read_env POSTGRES_PASSWORD)"

  [[ -z "$baked" ]] && { warn "could not read the DATABASE_URL password out of docker-compose.yml — skipping the divergence check"; return 0; }
  [[ -z "$env_pw" ]] && { ok "POSTGRES_PASSWORD unset in $ENV_FILE; compose default applies"; return 0; }

  if [[ "$env_pw" != "$baked" ]]; then
    fail "POSTGRES_PASSWORD in $ENV_FILE does not match the password hardcoded in
       docker-compose.yml's DATABASE_URL for the app services.

       crack-postgres would come up with YOUR password and every app service
       would keep trying the hardcoded one, so postgres looks healthy while the
       bot sits in a restart loop on 'password authentication failed'.

       Fix either side so they agree. The durable fix is to make compose build
       DATABASE_URL from \${POSTGRES_PASSWORD} instead of hardcoding it."
    return 0
  fi
  ok "database password agrees between $ENV_FILE and docker-compose.yml"
}

check_volumes() {
  # See guard 3 in the header.
  local missing=()
  for v in pgdata crack_data; do
    docker volume inspect "$v" >/dev/null 2>&1 || missing+=("$v")
  done
  if (( ${#missing[@]} )); then
    fail "external volume(s) missing on context '$CURRENT_CONTEXT': ${missing[*]}

       docker-compose.yml declares these 'external: true', so compose will not
       create them — a cold start on a fresh host fails here.

       Fix: docker volume create ${missing[*]}"
    return 0
  fi
  ok "external volumes present (pgdata, crack_data)"
}

check_discord_token() {
  # See guard 4 in the header.
  local tok
  tok="$(read_env DISCORD_TOKEN)"
  if [[ -z "$tok" || "$tok" == "XXXXXX" ]]; then
    fail "DISCORD_TOKEN is missing or still the placeholder in $ENV_FILE.

       The bot fail-closes at boot, not at deploy: this would not fail the
       deploy, it would replace a working container with one that panics on
       'Failed to load bot config' and restarts until it is given up on."
    return 0
  fi
  ok "DISCORD_TOKEN present"
}

preflight() {
  # Report everything, refuse nothing. Dying on the first failed guard turns
  # "your deploy is not ready" into one problem per invocation; this is the
  # subcommand whose entire job is to hand back the whole list at once.
  STRICT=0
  hdr "preflight — stack=$STACK env=$ENV_FILE context=$CURRENT_CONTEXT"

  if [[ -f "$ENV_FILE" ]]; then
    ok "$ENV_FILE present"
    check_discord_token
    check_db_password
  else
    fail "$ENV_FILE not found in $SCRIPT_DIR
       Copy .env.example to .env and fill it in, or set ENV_FILE=<path>."
    warn "skipping the DISCORD_TOKEN and database-password checks — both read $ENV_FILE"
  fi

  # Volume presence is context-specific, so it only means anything once the
  # context is the one a deploy would actually use.
  if [[ "$CURRENT_CONTEXT" == "$EXPECTED_CONTEXT" ]]; then
    ok "docker context is '$CURRENT_CONTEXT' as expected"
    check_volumes
  else
    fail "docker context is '$CURRENT_CONTEXT', expected '$EXPECTED_CONTEXT'
       State-changing subcommands will refuse until these agree.
       Fix:       DOCKER_CONTEXT=$EXPECTED_CONTEXT $0 $ORIGINAL_ARGS
       Intended?  EXPECTED_CONTEXT=$CURRENT_CONTEXT $0 $ORIGINAL_ARGS"
    warn "skipping the external-volume check — it is specific to the target context"
  fi

  echo
  if (( PROBLEMS )); then
    die "$PROBLEMS problem(s) above. Nothing was changed."
  fi
  ok "all guards passed"
  echo
}

# Everything that mutates the stack goes through here.
dc() {
  require_docker_context
  require_env_file
  # ENV_FILE is exported as well as passed: --env-file drives interpolation,
  # while the services' own `env_file:` entry reads ${ENV_FILE} so the file the
  # caller selected is the file whose variables reach the containers. Without
  # the export those two could disagree, and the containers would silently take
  # a different .env than the one this run was told to use.
  ENV_FILE="$ENV_FILE" docker compose -p "$STACK" --env-file "$ENV_FILE" "$@"
}

# Read-only compose invocation. Deliberately does NOT require the context or
# the env file: `ps` against the wrong host shows nothing and `logs` without a
# .env still tails fine, and refusing to SHOW you the state of things is a poor
# trade for a mistake you cannot make by looking. Both facts are warned about
# and both appear in the banner.
dc_ro() {
  local envargs=()
  if [[ -f "$ENV_FILE" ]]; then
    envargs=(--env-file "$ENV_FILE")
  else
    warn "$ENV_FILE not found — reading without it; docker-compose.yml's \${VAR:-default} values apply" >&2
  fi
  if [[ "$CURRENT_CONTEXT" != "$EXPECTED_CONTEXT" ]]; then
    warn "docker context is '$CURRENT_CONTEXT', not '$EXPECTED_CONTEXT' — this reads THAT host" >&2
  fi
  ENV_FILE="$ENV_FILE" docker compose -p "$STACK" ${envargs[@]+"${envargs[@]}"} "$@"
}

banner() { printf "[cracktunes] stack=%s env=%s context=%s — %s\n" "$STACK" "$ENV_FILE" "$CURRENT_CONTEXT" "$*"; }

# ------------------------------------------------------------ subcommands ----

cmd_up() {
  require_env_file; check_discord_token; check_db_password
  require_docker_context; check_volumes
  banner "up"
  dc up -d "$@"
}

cmd_deploy() {
  require_env_file; check_discord_token; check_db_password
  require_docker_context; check_volumes
  banner "pulling fresh images and recreating ${*:-all services}"
  dc pull "$@"
  dc up -d --force-recreate "$@"
}

cmd_restart() {
  banner "recreating ${*:-all services} WITHOUT pulling"
  echo "[cracktunes] NOTE: this ships no new code. If a new image was published,"
  echo "[cracktunes] use '$0 deploy' instead — the services pin floating tags and"
  echo "[cracktunes] set no pull_policy, so a plain recreate keeps the cached image."
  dc up -d --force-recreate "$@"
}

cmd_stop() { banner "stop"; dc stop; }
cmd_down() { banner "down (volumes preserved)"; dc down; }
cmd_ps()   { dc_ro ps; }
cmd_logs() { dc_ro logs -f --tail=50 "$@"; }

cmd_destroy() {
  echo "[cracktunes] 'compose down -v' DROPS the postgres volume: guild settings,"
  echo "[cracktunes] playlists, play logs, user votes — all of it."
  read -r -p "Type the stack name ($STACK) to confirm: " confirm
  [[ "$confirm" == "$STACK" ]] || die "confirmation mismatch — aborting"
  dc down -v
}

cmd_shell() {
  local svc="${1:-}"
  [[ -n "$svc" ]] || die "usage: $0 shell <service>"
  dc exec "$svc" sh
}

_pg_user() { local u; u="$(read_env POSTGRES_USER)"; printf '%s' "${u:-postgres}"; }
_pg_db()   { local d; d="$(read_env POSTGRES_DB)";   printf '%s' "${d:-postgres}"; }

cmd_psql() {
  # Unix socket inside the container, so no password is needed from outside.
  dc exec crack-postgres psql -U "$(_pg_user)" -h /var/run/postgresql -d "$(_pg_db)"
}

cmd_sql() {
  # Read-only is enforced by the SERVER, not by trusting the SQL on stdin —
  # this exists to be driven by tooling, where "we only send SELECTs" is not a
  # strong enough guarantee. -tAX makes the output parseable.
  dc exec -T -e PGOPTIONS='-c default_transaction_read_only=on' crack-postgres \
    psql -U "$(_pg_user)" -h /var/run/postgresql -d "$(_pg_db)" \
      -tAX -v ON_ERROR_STOP=1 -P pager=off "$@"
}

cmd_migrate() {
  require_env_file
  command -v sqlx >/dev/null 2>&1 || die "sqlx not found in PATH.
       Install: cargo install sqlx-cli --no-default-features --features rustls,postgres"
  local url
  url="$(read_env DATABASE_URL)"
  [[ -n "$url" ]] || die "DATABASE_URL not set in $ENV_FILE"
  banner "running migrations"
  DATABASE_URL="$url" sqlx migrate run --source migrations/
}

cmd_status() {
  hdr "stack — $STACK on context $CURRENT_CONTEXT"
  dc_ro ps || true
  hdr "images"
  dc_ro config --images 2>/dev/null || true
  hdr "bot version"
  # The container has no shell-free version flag; read the label off the image
  # it is actually running, which is what matters after a deploy.
  local cid
  cid="$(dc_ro ps -q cracktunes 2>/dev/null | head -1)"
  if [[ -n "$cid" ]]; then
    docker inspect --format '{{.Config.Image}}  (started {{.State.StartedAt}})' "$cid" 2>/dev/null || true
  else
    warn "cracktunes container is not running"
  fi
  echo
}

usage() {
  # Print the header comment block, stripped of the '# ' prefix. Anchored on the
  # first code line so it survives edits without hardcoded line numbers.
  sed -n '3,/^set -euo pipefail/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

case "${1:-help}" in
  preflight)  preflight ;;
  up)         shift; cmd_up "$@" ;;
  deploy)     shift; cmd_deploy "$@" ;;
  restart)    shift; cmd_restart "$@" ;;
  stop)       cmd_stop ;;
  down)       cmd_down ;;
  destroy)    cmd_destroy ;;
  ps)         cmd_ps ;;
  logs)       shift; cmd_logs "$@" ;;
  shell)      shift; cmd_shell "$@" ;;
  psql)       cmd_psql ;;
  sql)        shift; cmd_sql "$@" ;;
  migrate)    cmd_migrate ;;
  status)     cmd_status ;;
  help | -h | --help) usage ;;
  *)
    echo "[cracktunes] unknown subcommand: $1" >&2
    usage
    exit 2
    ;;
esac
