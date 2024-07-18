#!/bin/bash

SERVICE_NAME="crack-voting"

# TODO: Maybe build first?

# Pull the latest image.
docker compose pull ${SERVICE_NAME}

# Recreate the specific service without affecting its dependencies
docker compose up -d --no-deps --force-recreate ${SERVICE_NAME}
