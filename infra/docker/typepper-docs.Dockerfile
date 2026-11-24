# typepper-docs: docs.typepper.com, static. Railway builds it from the repo root
# (see infra/railway):
#   docker build -f infra/docker/typepper-docs.Dockerfile .
# Bases are pinned by digest (dependabot bumps them; docs.yml builds this, so
# a bump is tested). mise.toml's Zola is only for `mise run docs`.

# docs/ → site/content/, links turned into ones Zola checks (site/prepare.py)
FROM python:3.15-alpine@sha256:0f9552bb28711333da16915ec146f5e6bb44a7fdac9ced15dc17c2e0206346dd AS prepare
WORKDIR /src
COPY . .
RUN python3 site/prepare.py

# a broken link between docs fails here, and the deploy with it
FROM ghcr.io/getzola/zola:v0.23.6@sha256:722c7af3c6a7c0ccc37f01d85d47ee3e8b53ea6399f2b7dd7f9b82af7624fd61 AS build
COPY --from=prepare /src/site /site
RUN ["/zola", "--root", "/site", "build"]

FROM joseluisq/static-web-server:2@sha256:ccf2d666ffe5f8ef2ec96bc40f07757605998c04b78f7d8fdb98a584dd4f5717
COPY --from=build /site/public /public
# PORT is Railway's: its domain and health check go there
ENV SERVER_ROOT=/public SERVER_PORT=8080
EXPOSE 8080
