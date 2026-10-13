# Archive access tracking and security webhooks

Tracking is an optional property of an identity-protected `.lttl` archive. It is off by default. A sender can enable it, or a broker administrator can require it for newly created archives. It is not a recipient client preference. ZIP/TAR files without the encrypted identity envelope cannot reliably enforce phone-home and do not support this feature.

The encrypted header carries the effective policy, including country watchlists. Its authentication tag prevents undetected policy changes. The broker also keeps an immutable policy against the archive ID, so an altered client/header cannot turn off server-side logging. An existing archive's policy is not changed retroactively by a new organization default. Recreate it to change the policy. Older lttl clients cannot read tracked headers; upgrade recipients before distributing these archives.

## Sender controls

In **Create**, select **Encrypt for selected organization users**, choose recipients, then select **Track access attempts for this archive**. Optional alert country codes are comma-separated; `KP` is North Korea. Organization-required tracking and country watchlists are merged by the broker and saved in the resulting archive. A request fails if tracking is selected but the organization has not configured a webhook.

```powershell
lttl create .\documents -o .\shared.lttl --identity-profile .\company.json --recipient RECIPIENT_BROKER_ID --track-access --alert-country KP
lttl identity events .\shared.lttl --profile .\company.json
```

The second command requires the archive creator's fresh management sign-in. It returns the newest 200 retained events. Recipients and anonymous callers cannot read an archive's activity. The configured organization webhook receives all tracking events; use its `alerts` array to route notifications in your security system.

## What is reported

| Event | Meaning |
|---|---|
| `open_attempt` | A valid sign-in request referenced this tracked archive; identity is not yet known |
| `authenticated` | The broker verified a fresh IDP token and enrolled/resolved its issuer and subject |
| `authentication_failed` | A known sign-in flow returned an IDP refusal or failed verification; identity remains unknown |
| `key_denied` | A request for the archive key was denied; identity is reported only if its session was verified |
| `key_released` | The broker authorized a key response after recording the event; this is not confirmation of successful extraction |

Events contain a stable event ID, archive ID, Unix UTC timestamp, verified broker user ID/IDP issuer/subject when available, organization-membership classification, observed network address, estimated country and evidence source. They do not contain archive contents, file names, passwords, tokens, encryption keys, machine identifiers or GPS coordinates. The client displays disclosure before opening; it contacts only its explicitly configured trusted broker, never an arbitrary tracker embedded in a file.

Alerts are `outside_organization`, `access_denied`, `authentication_failed` and `country_watchlist`. They indicate observations requiring review, **not proof of theft**. Anonymous requests can reference a known archive ID without possessing the file. Offline copies, blocked network requests, abandoned IDP pages, retained plaintext/keys and modified clients that already possess a key cannot be fully observed. A denied user cannot obtain a new key, but the broker cannot revoke plaintext already obtained.

## Broker configuration

Upgrade the broker first. Run its existing `--init-db CONFIG.json` migration command against managed PostgreSQL before serving traffic; it adds tracking policy, events and outbox tables without replacing existing archives. SQLite development databases add the tables at startup. Back up production state before any deployment.

Add this block to the existing broker JSON:

```json
{
  "tracking": {
    "webhook_url": "https://security.example.com/lttl-events",
    "require_tracking": false,
    "alert_countries": ["KP"],
    "organization_member_ids": null,
    "signed_geo_headers": false
  }
}
```

`webhook_url` is configured by the broker operator, not by archive authors. HTTPS is required except loopback tests; redirects are not followed. Supply separate random secrets of at least 32 bytes through the existing Azure/AWS/GCP secret-store/environment mechanism:

- `LTTL_TRACKING_WEBHOOK_SECRET`: shared only with the receiver, to sign deliveries.
- `LTTL_TRACKING_DISPATCH_TOKEN`: used by the organization's scheduled retry caller.
- `LTTL_TRACKING_GEO_SECRET`: required only when signed gateway location enrichment is enabled; keep it separate from the other secrets.

`require_tracking: true` enforces tracking on newly created archives, even if the sender doesn't select the checkbox. `alert_countries` adds organization watchlist codes to enabled archive policies. Country codes must be uppercase two-letter codes.

`organization_member_ids` is an optional, explicit list of **lttl broker account IDs** obtained after enrollment using `lttl identity users`. An authenticated account not in the configured list is classified as external. `null` means membership is unknown; an empty list classifies all authenticated accounts as external. Update this list as membership changes and deploy consistently to all instances. This version does not automatically synchronize IDP groups or infer membership from email domains. Single-tenant IDPs may deny outside accounts before the broker can identify them; those events remain anonymous authentication failures. IDP sign-in logs can supply additional context.

## Signed webhook protocol

Each POST contains the JSON event as its exact UTF-8 body, with:

```text
X-Lttl-Event-Id: stable event ID
X-Lttl-Timestamp: delivery Unix UTC seconds
X-Lttl-Signature: base64url without padding of HMAC-SHA256
```

The signed bytes are `event_id + "." + timestamp + "." + raw_body`. Verify with `LTTL_TRACKING_WEBHOOK_SECRET` using a constant-time comparison, require a recent delivery timestamp (for example within five minutes), and deduplicate by event ID. Compare the header event ID to the JSON event ID after verifying. Return a 2xx response after durably accepting the event. Retries reuse the event ID and receive a new delivery timestamp/signature. The receiver decides how to notify analysts/users; lttl does not directly send email. See [the runnable receiver example](../scripts/tracking-webhook-receiver.py).

Events and outbox entries are committed in one database transaction before key responses. A webhook outage does not block access once the event is durable; database recording failure does block the response. Each event attempts one bounded delivery (three-second HTTP timeout). Failed deliveries retry with increasing delays, capped at six hours. Concurrent instances acquire a database lease; delivery is **at least once**, so duplicates are possible after a crash. HTTP redirects, 4xx and 5xx remain failures.

Configure a scheduled HTTPS POST to `/v1/tracking/dispatch` with `Authorization: Bearer LTTL_TRACKING_DISPATCH_TOKEN`, typically every minute. The endpoint attempts up to ten due events and returns `delivered` and `pending` counts. Use an Azure timer-triggered Function, AWS EventBridge Scheduler with a Lambda caller, or Google Cloud Scheduler with an organization-controlled authenticated caller. These remain serverless; no VM is required. Store the dispatch token in your platform's secret store. Platform IAM alone does not replace this endpoint's token. Match the scheduler timeout to a possible 30-second batch and monitor pending counts/retry age in PostgreSQL. For volume, invoke additional bounded batches and apply ingress rate limits, particularly to anonymous attempts.

The scheduled dispatcher also deletes events older than **90 days**, cascading their outbox rows, including undelivered events. Schedule it even when traffic is idle; without invocation, cleanup does not run. Receiver/SIEM retention is configured separately. No webhook, country lookup or scheduled caller is provisioned by the desktop installer.

## Trusted country estimates

By default the broker records its transport peer address and **unknown country**. Behind serverless ingress that peer can be an internal proxy. It never trusts client-provided `X-Forwarded-For` or country headers by themselves.

To enable country alerts, deploy a trusted ingress/gateway that determines the original address from its platform's authoritative request metadata and resolves country through your organization's maintained IP-location database/service. Strip caller-supplied tracking headers. Set `signed_geo_headers: true`, then send:

```text
X-Lttl-Client-Ip: one parsed IPv4/IPv6 address
X-Lttl-Country: two-letter uppercase country, or - for unknown
X-Lttl-Geo-Timestamp: current Unix UTC seconds
X-Lttl-Geo-Signature: base64url HMAC-SHA256, no padding
```

Sign the UTF-8 string `timestamp + "\n" + HTTP_METHOD + "\n" + request_path_without_query + "\n" + ip + "\n" + country` with the geo secret. The broker accepts signatures within 60 seconds, bound to method/path. Missing, forged, stale or malformed enrichment falls back to transport-peer/unknown-country data. Apply the gateway to sign-in start, browser callback and key endpoints; the browser and app can have different network paths. No generic cloud header is assumed to be authoritative. A compromised trusted gateway can falsify this metadata.

Country is a network estimate, not a user's physical location. VPNs/proxies can present another country's address; Microsoft documents the same limitation for [IP-based Conditional Access locations](https://learn.microsoft.com/en-us/entra/identity/conditional-access/concept-assignment-network). Watchlists **alert only**; use IDP Conditional Access or equivalent policy for access blocking. No country restriction is hard-coded.

## Validation

Run broker tests and `python scripts/test-identity-flow.py --tracking --cli PATH_TO_LTTL --broker PATH_TO_BROKER`. Tests use disposable SQLite, a local RSA-signed mock IDP and a local webhook receiver. They cover policy off/on/org enforcement, authenticated headers, owner isolation, outsider/denial alerts, unknown identities, signed country estimates, webhook HMAC verification and outage/restart retry. Live cloud gateways, SIEM routing, tenant policy and scheduler delivery still require organization pilot testing. The feature is included in the alpha.2 Windows installer. Upgrade the broker and initialize its schema before using the new desktop client.

[Recorded end-to-end results](tracking-validation.json) and [PostgreSQL/TLS checks](identity-postgres-validation.json) are checked into the repository. The tracking storage-failure test ensures a key is not returned when its tracking event cannot be committed.
