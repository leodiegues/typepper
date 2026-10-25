// typepper on Railway: the server (HTTP + SSH), its Postgres, and the docs.
// `mise run infra:plan` / `infra:apply` / `infra:deploy` (see mise.toml);
// .github/workflows/deploy-production.yml runs apply then deploy.
// No GitHub source on purpose: Railway never deploys on its own, only CI does.
import { defineRailway, postgres, preserve, project, service } from "railway/iac";

export default defineRailway(() => {
  // limits as set in the dashboard: 2 vCPU, 2 GB, like the server. postgres()
  // takes no deploy options, but its node carries them through to the plan
  const limits = { limitOverride: { containers: { cpu: 2, memoryBytes: 2_000_000_000 } } };
  const db = Object.assign(postgres("typepper-postgres"), { deploy: limits });

  const server = service("typepper-server", {
    build: {
      builder: "DOCKERFILE",
      dockerfilePath: "infra/docker/typepper-server.Dockerfile",
    },
    deploy: limits,
    // traffic moves to a new deploy only once this answers 200 (it checks Postgres)
    healthcheck: "/health",
    // IaC can't register custom domains: they were added with
    // `railway domain <name> -p 8080 -s typepper-server`, and are listed here so
    // applies keep them
    domains: [
      { domain: "api.typepper.com", port: 8080 },
      { domain: "r.typepper.com", port: 8080 },
    ],
    // ssh.typepper.com: Railway picks the public port, and released games keep
    // it, so this proxy must never be removed
    tcp: [2222],
    env: {
      PORT: "8080", // Railway's own: its domains and health check go to PORT
      TYPEPPER_SSH_PORT: "2222",
      TYPEPPER_DATABASE_URL: db.env.DATABASE_URL,
      // an OpenSSH private key, set once in Railway; the game ships its public half
      TYPEPPER_SSH_HOST_KEY: preserve(),
    },
  });

  // docs.typepper.com: static, built from docs/. Its own service, so a docs
  // deploy never restarts the server (and the SSH sessions it holds)
  const docs = service("typepper-docs", {
    build: {
      builder: "DOCKERFILE",
      dockerfilePath: "infra/docker/typepper-docs.Dockerfile",
    },
    healthcheck: "/",
    // docs.typepper.com: register it with `railway domain docs.typepper.com
    // -p 8080 -s typepper-docs`, then list it here like the server's
    env: {
      PORT: "8080",
    },
  });

  return project("typepper", { resources: [server, db, docs] });
});
