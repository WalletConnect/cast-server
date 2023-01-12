# Rust HTTP Server Starter

This is a templated based on [Echo Server](https://github.com/WalletConnect/echo-server) and [Bat Cave](https://github.com/WalletConnect/bat-cave) that
contains the basic setup of a rust HTTP Server with telemetry and a postgres database.

There is also a basic terraform config included to be built upon.

This project also includes the standard CI/CD:
- Release
- Rust CI
- Terraform CI
- CD
- Intake
- Mocha (NodeJS) based integration tests

## Running the app

* Build: `cargo build`
* Test: `cargo test`
* Run: `docker-compose-up`
* Integration test: `yarn install` (once) and then `yarn integration:local(dev/staging/prod)`



## Required Values to Change
Any reference to `rust-http-starter` should be changed to your project name as well as all the below list:

- [ ] `README.md`
  Replace this file with a Readme for your project
- [ ] `Cargo.toml`
  Change package name and authors, then build the project to re-gen the `Cargo.lock`
- [ ] `/terraform/main.tf`
  Change `app_name` to the repo's name
- [ ] `/terraform/main.tf`
  Setup a new hosted zone in the [infra repository](https://github.com/WalletConnect/infra/blob/master/terraform/main.tf#L123)
- [ ] `/terraform/variables.tf`
  Change the default value of `public_url`
- [ ] `/terraform/backend.tf`
  Change the `key` for the `s3` backend to your project's name
- [ ] `/.github/workflows/cd.yml`
  Ensure any URLs for health checks and environments match the terraform files
- [ ] `/.github/workflows/intake.yml`
  This is specific to WalletConnect, feel free to remove or modify for your own needs
- [ ] `/.github/ISSUE_TEMPLATE/feature_request.yml`
  Change any references to Rust HTTP Starter\
- [ ] `/.github/workflows/release.yml`
  On line 95-97 there are references to the registry name on ECR/GHCR ensure you change this
- [ ] `/.github/integration/integration.tests.ts`
  Update the URLs

### WalletConnect Specific

- [ ] `/.github/workflows/**/*.yml`
  Change the `runs-on` to the `ubuntu-runners` group

## GitHub Secrets
Required GitHub secrets for Actions to run successfully
- `AWS_ACCESS_KEY_ID`
- `AWS_SECRET_ACCESS_KEY`
- `PAT` Personal Access Token for Github to commit releases

### WalletConnect Specific
- `ASSIGN_TO_PROJECT_GITHUB_TOKEN`
