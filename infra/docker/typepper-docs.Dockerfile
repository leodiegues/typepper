# typepper-docs: docs.typepper.com, static. Railway builds it from the repo root
# (see infra/railway):
#   docker build -f infra/docker/typepper-docs.Dockerfile .
# Bases are pinned by digest (dependabot bumps them; docs.yml builds this, so
# a bump is tested). mise.toml's Zola is only for `mise run docs`.

# docs/ → site/content/, links turned into ones Zola checks (site/prepare.py)
FROM python:3.14-alpine@sha256:601e99e090cb08c2784a42395487709500bc39930307fe72f3f11a8f3c430762 AS prepare
WORKDIR /src
COPY . .
RUN python3 site/prepare.py

# a broken link between docs fails here, and the deploy with it
FROM ghcr.io/getzola/zola:v0.23.6@sha256:722c7af3c6a7c0ccc37f01d85d47ee3e8b53ea6399f2b7dd7f9b82af7624fd61 AS build
COPY --from=prepare /src/site /site
RUN ["/zola", "--root", "/site", "build"]

FROM joseluisq/static-web-server:2@sha256:94618e3d17802265a21926217e6f9e7832b311b6fbf5933684001739fafed06a
COPY --from=build /site/public /public
# PORT is Railway's: its domain and health check go there
ENV SERVER_ROOT=/public SERVER_PORT=8080
EXPOSE 8080
