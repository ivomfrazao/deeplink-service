# Resolution Flow

This document defines the flow for resolving deep links using the Deeplink Service. Each `break` block is an early exit: the request stops there with the shown response. Checks run in the order shown (see [Architecture § Resolution](../architecture.md#resolution)).

```mermaid
sequenceDiagram
    participant Audience as Audience App
    participant IdP as IdP
    participant API as API Layer
    participant CA as Client Auth
    participant UA as User Auth
    participant CORE as DeepLink Logic
    participant REPO as Repository / DB
    participant IDEM as Idempotency Store

    opt Link URL has private=true
        Audience->>IdP: Login + fetch user access token (aud = Deeplink Service)
    end
    Audience->>API: POST /api/v1/links/{id}/resolve

    API->>CA: Validate client credentials
    break invalid credentials
        API-->>Audience: 401 Unauthorized (INVALID_CLIENT_CREDENTIALS)
    end

    API->>CORE: Resolve link

    opt Idempotency-Key present
        CORE->>IDEM: Look up (audience client, key)
        break key stored for a different link
            API-->>Audience: 422 Unprocessable Entity (IDEMPOTENCY_KEY_REUSED)
        end
    end

    CORE->>REPO: Load link by id
    break not found or audience mismatch
        API-->>Audience: 404 Not Found (LINK_NOT_FOUND)
    end
    break not a replay, and link expired, exhausted, or revoked
        API-->>Audience: 404 Not Found (LINK_NOT_FOUND)
    end

    opt Private link (userSub set)
        break user token missing
            API-->>Audience: 401 Unauthorized (USER_TOKEN_REQUIRED)
        end
        CORE->>UA: Validate user token (JWKS, aud)
        break invalid user token
            API-->>Audience: 401 Unauthorized (INVALID_USER_TOKEN)
        end
        UA-->>CORE: sub
        break sub != link userSub
            API-->>Audience: 403 Forbidden (USER_MISMATCH)
        end
    end
    Note over CORE: Public link: user token header ignored, not validated

    alt Idempotency-Key replay (stored outcome found)
        CORE->>IDEM: Load stored outcome
    else First request
        CORE->>REPO: Increment useCount, set lastUsedAt (conditional on still usable), load userContext, write "link resolved" audit event
        break conditional update matched no row (lost a race)
            API-->>Audience: 404 Not Found (LINK_NOT_FOUND)
        end
        opt Idempotency-Key present
            CORE->>IDEM: Store outcome (link id, sub, response)
        end
    end

    CORE-->>API: userContext
    API-->>Audience: 200 OK
```
