# Deeplink Service

A backend that issues and resolves secure, single‑use deep links between client applications.

## Objectives

The objectives are:

- Provide a centralized service that generates audience‑specific URLs so client applications do not need to know or maintain each other’s endpoints.
- Allow passing arbitrary user context data at creation time, which is then securely stored and made available to the audience client upon resolution.
- Ensure that only the intended audience client can resolve the link and, optionally, only the intended user can use it if it's private.
- Enforce single‑use and expiration policies to enhance security.
- Support multiple audiences, scopes, and platforms through a flexible mapping system.

### Assumptions

- All parties use the same IdP, so:
  - Deeplink Service can validate user tokens
  - Clients can fetch user tokens from IdP
  - No cross‑IdP token translation is needed
- The IdP issues user access tokens with an `aud` claim that includes the Deeplink Service, and creator/audience apps know to request tokens scoped accordingly when fetching a user token for a private link (see [Architecture](docs/architecture.md#token-validation)) — a token scoped only to another resource server (e.g. the audience app's own API) will be rejected
- All audience clients have a user‑facing component to handle the deep link and prompt for login if needed (for private links)

### Out-of-scope

The service does not support:

- Multi‑IdP between the clients and the service itself.
- Actual redirection of users. It only provides the target URL and context. Client applications handle the redirection and user experience.

## Documentation structure

The documentation is organized into the following sections:

- [Architecture](docs/architecture.md)
- [Data Model](docs/data_model.md)
- [Flows](docs/flows/flows.md)
- [API Reference](docs/api_v1.md)
- [Implementation Notes](docs/implementation_notes.md) — design reasoning and rejected alternatives, for context beyond the formal spec
- [Future Improvements](docs/future_improvements.md) — deferred work and open decisions
