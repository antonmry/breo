# GitHub Pages Deployment

This directory contains the GitHub Actions workflow for deploying the Browser PDS web application to GitHub Pages.

## Workflow: `deploy-pages.yml`

### Triggers

- **Push to main**: Builds and deploys to GitHub Pages
- **Pull Requests**: Builds the app and provides artifact for review
- **Manual**: Can be triggered manually via workflow_dispatch

### Jobs

#### 1. Build
Runs on all triggers (main push, PRs, manual):
- Builds WASM package using wasm-pack
- Installs Node.js dependencies
- Builds the webapp with Vite
- Uploads build artifact

#### 2. Deploy
Runs only on push to main:
- Deploys the built artifact to GitHub Pages
- Sets up the GitHub Pages environment
- Provides deployment URL

#### 3. Preview
Runs only on pull requests:
- Downloads the build artifact
- Comments on the PR with build status
- Provides instructions for testing

## Setup Required

### One-Time Setup

1. **Enable GitHub Pages in repository settings:**
   - Go to Settings > Pages
   - Source: GitHub Actions
   - This allows the workflow to deploy

2. **Set up environment (optional but recommended):**
   - Go to Settings > Environments
   - Create environment named `github-pages`
   - Add protection rules if desired (e.g., required reviewers)

### How It Works

#### For Main Branch
```
Push to main → Build WASM → Build Webapp → Deploy to Pages → Live at username.github.io/repo-name
```

#### For Pull Requests
```
Open PR → Build WASM → Build Webapp → Upload Artifact → Comment on PR
```

The PR comment includes:
- Build status
- Instructions to download and test locally
- Link to workflow run
- Note that deployment only happens on merge

## Testing PR Changes

When a PR is created or updated:

1. The workflow builds the webapp
2. Build artifacts are available in the workflow run
3. A comment is posted on the PR with instructions
4. Download the artifact to test locally:
   ```bash
   # Download artifact from Actions tab
   unzip github-pages.zip
   
   # Serve locally
   python -m http.server 8000
   # or
   npx serve .
   ```

## Deployment URL

Once deployed to main, the site will be available at:
```
https://<username>.github.io/<repository-name>/
```

For this repository:
```
https://antonmry.github.io/pds-wasm/
```

## Caching

The workflow uses caching for:
- Cargo registry and build artifacts
- Node.js dependencies (npm cache)

This speeds up subsequent builds significantly.

## Security

The workflow uses:
- `contents: read` - To checkout code
- `pages: write` - To deploy to GitHub Pages
- `id-token: write` - For GitHub Pages deployment
- `pull-requests: write` - To comment on PRs

## Concurrency

Only one deployment can run at a time per branch/PR to avoid conflicts.
New runs will cancel in-progress ones for the same ref.
