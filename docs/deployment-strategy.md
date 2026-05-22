# Deployment Strategy

The wallet service follows BidMart's staging-first progressive promotion strategy.

## Environment Mapping

| Branch | Environment | Platform |
| --- | --- | --- |
| `staging` | Staging | VPS Docker Compose through `bidmart-infrastructure` |
| `main` | Production | VPS Docker Compose through `bidmart-infrastructure` |

## Gate

The service dispatches VPS deployment only after the `Continuous Integration` workflow succeeds. This keeps wallet changes from reaching staging or production if tests fail.

## Promotion Flow

1. Merge wallet changes into `staging`.
2. CI validates Rust tests and static analysis.
3. Successful CI dispatches staging deployment in `bidmart-infrastructure`.
4. Validate payment and hold flows in staging.
5. Promote the same change to `main`.
6. CI success on `main` dispatches production deployment.

## Rollback

Rollback is done by reverting the production branch and allowing CI to trigger a fresh deployment. Manual production redeploy can use a known-good branch through the infrastructure workflow.
