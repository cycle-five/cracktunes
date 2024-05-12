# Build image
# Necessary dependencies to build CrackTunes
FROM debian:bookworm-slim as build
ARG SQLX_OFFLINE=true

RUN apt-get update && apt-get install -y \
       autoconf \
       automake \
       cmake \
       libtool \
       libssl-dev \
       pkg-config \
       libopus-dev \
       curl \
       git

# Get Rust
RUN curl -proto '=https' -tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
       && . "$HOME/.cargo/env" \
       && rustup default stable

WORKDIR "/app"

COPY . .
COPY names.txt /app/names.txt
RUN . "$HOME/.cargo/env" && cargo build --release --locked

# Release image
# Necessary dependencies to run CrackTunes
FROM debian:bookworm-slim AS runtime

ARG USERNAME=cyclefive
ARG USER_UID=1000
ARG USER_GID=$USER_UID
ENV HOME=/home/${USERNAME}

VOLUME [ "/data" ]
RUN mkdir -p /data && chown -R ${USER_UID}:${USER_GID} /data
# VOLUME [ "/var/lib/postgresql/data" ]

# Update the package list, install sudo, create a non-root user, and grant password-less sudo permissions
RUN groupadd --gid $USER_GID $USERNAME \
       && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
       #
       # [Optional] Add sudo support. Omit if you don't need to install software after connecting.
       && apt-get update \
       && apt-get install -y sudo \
       && echo $USERNAME ALL=\(root\) NOPASSWD:ALL > /etc/sudoers.d/$USERNAME \
       && chmod 0440 /etc/sudoers.d/$USERNAME

USER $USERNAME

RUN sudo apt-get update \
       # && apt-get upgrade -y \
       && sudo apt-get install -y ffmpeg curl \
       # Clean up
       && sudo apt-get autoremove -y \
       && sudo apt-get clean -y \
       && sudo rm -rf /var/lib/apt/lists/*

#RUN sudo curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp/releases/download/2024.04.09/yt-dlp_linux \
RUN sudo curl -sSL --output /usr/local/bin/yt-dlp https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/download/2024.05.11.232654/yt-dlp_linux \
       && sudo chmod +x /usr/local/bin/yt-dlp



RUN yt-dlp -v -h

# USER 1000
WORKDIR "${HOME}/app"

COPY --chown=${USER_UID}:${USER_GID} --from=build /app/target/release/cracktunes $HOME/app/cracktunes
COPY --chown=${USER_UID}:${USER_GID} --from=build /app/data  $HOME/app/data
COPY --chown=${USER_UID}:${USER_GID} --from=build /app/.env.example $HOME/app/.env
COPY --chown=${USER_UID}:${USER_GID} --from=build /app/cracktunes.toml $HOME/app/cracktunes.toml
COPY --chown=${USER_UID}:${USER_GID} --from=build /app/names.txt $HOME/app/names.txt
# RUN ls -al / && ls -al /data

ENV APP_ENVIRONMENT production
RUN . "$HOME/app/.env"
ENV DATABASE_URL postgresql://postgres:mysecretpassword@localhost:5432/postgres
CMD ["/home/cyclefive/app/cracktunes"]
