# TODO

## Remaining Human Requirements (Manual Setup)

### 1. GitHub & Git Distribution
- [ ] Create a GitHub Personal Access Token (PAT) with write access to this repository and each external subrepo.

### 2. Infrastructure & Production Secrets
- [ ] Provision a **Turso** database for the production control plane.
- [ ] Configure production secrets for the Cloudflare Worker:
  - `TURSO_DB_URL`
  - `TURSO_DB_AUTH_TOKEN`
- [ ] Provision a **GitHub App** for the production environment.
- [ ] Configure GitHub App credentials:
  - `GITHUB_APP_ID`
  - `GITHUB_PRIVATE_KEY`

## Why these remain

- This local agent environment cannot create GitHub secrets or provision external repositories.
- Production infrastructure (Turso/Cloudflare) and GitHub App configuration require human credentials and dashboard access.
- External repository URLs were not provided, so `.gitrepo` metadata could not be finalized.
