# Status journal — 2026-W37

Journal entries for ISO week 2026-W37, newest first, in the order they were written.

> **2026-09-07 — [#916 "#904 follow-ups (silent refresh + ?next=): latch the one-shot on failure
> not use, `:param` routes in safe_next, wasm client timeouts, the same-tab captured `next`,
> `auth_refresh_total{outcome}` server-side"](https://github.com/TheCaptainCompany/captain-food/issues/916)
> item 2, first half only, draft [PR #948](https://github.com/TheCaptainCompany/captain-food/pull/948),
> Lane B (session_01H3AFBVzhSiGXJcFuwKjiMQ, `916-safe-next-param-routes`).** `safe_next` no longer
> refuses a `?next=` candidate that resolves to a `:param` route: it walks the SCREEN'S ROUTE
> TEMPLATE segment by segment — a literal segment copied verbatim, a `:name` segment looked up BY
> NAME among the resolved route's captured params (never iterated in capture order, since
> `resolve()` injects a storefront `slug` that has no place in a template lacking its own `:slug`
> segment) and percent-encoded per segment for the rebuilt path. A new `ReturnTarget` newtype,
> minted only by `safe_next` and `ReturnTarget::root`, carries the "came from the router" guarantee
> `'static` used to (a rebuilt param path is no longer a slice of the screen's own route). `safe_next`
> now returns `Result<ReturnTarget, NextRejection>`; `NextRejection` is a closed set
> (`Shape | Unresolved | OpenScreen | Placeholder`). A captured value that is itself the unresolved
> template placeholder text (only reachable percent-encoded, e.g. `%3AorderId`) is rejected
> (`Placeholder`) rather than rebuilt into a broken destination. No existence check of the
> destination resource is ever performed — the destination screen resolves the id at the point of
> need, same as any other navigation.
>
> **Lens splits and how resolved** (ADR-20260904-013834, the team decides): the placeholder question
> split reviewer and holub (reject any candidate touching the placeholder text at all, the more
> defensive read) against evans and graphql-architect (reject only when the CAPTURED value equals
> the placeholder — a real value can legitimately equal arbitrary bytes) — the SAFER side was taken:
> `Placeholder` fires whenever a captured value starts with `:`, since the router's own SHAPE check
> already refuses a raw `:` before decoding, so the only way a captured value can start with `:`
> post-decode is the placeholder-bypass case this guard exists for. holub's separate objection —
> that `NextRejection`'s four-variant closed set grows the type surface for a `{outcome}` attribute
> nothing consumes yet — is recorded, not overridden: the set stays because the alternative (a bare
> `Option`, then a rewrite when the counter lands) costs more than the dozen lines now. holub also
> banked, at the checkpoint, that `ReturnTarget` itself is a FORCED compiler-first consequence, not
> a design point the consent text enumerated as decided: once a `:param` rebuild is no longer a
> `&'static str` slice of the screen's own route, SOME owned-string wrapper is unavoidable, and the
> newtype-with-private-constructor shape was the compiler-first floor (PROP-20260802-130500 §1)
> applied to that forced fact, not an independently chosen design point.
>
> **D1 — six base reds** (captured against a temporary stub that ported the OLD collapsed
> `Option`-returning behaviour into the NEW `Result`/newtype shape, so the failures were
> behavioural, not type errors — beck: "the honest form of make-it-compile-then-watch-it-fail"; a
> stricter reading held that every guard should be seen red under its OWN mutant too, which is why
> M6 exists separately below):
> ```text
> every_generated_route_is_reachable_and_unknown_paths_are_none: order_tracking (/orders/:orderId/confirmation)
>   left: Err(OpenScreen)
>  right: Ok(ReturnTarget("/orders/x/confirmation"))
>
> safe_next_boundaries_after_decode: the query split after decode is lossy but safe -- unchanged pre-existing behaviour
>   left: Err(OpenScreen)
>  right: Ok(ReturnTarget("/jobs/a"))
>
> safe_next_re_encodes_every_captured_segment: a space round-trips encoded
>   left: Err(OpenScreen)
>  right: Ok(ReturnTarget("/jobs/a%20b"))
>
> safe_next_rebuilds_a_param_route_from_its_captured_segments:
>   left: Err(OpenScreen)
>  right: Ok(ReturnTarget("/jobs/abc"))
>
> safe_next_rejects_the_template_placeholder:
>   left: Err(OpenScreen)
>  right: Err(Placeholder)
>
> safe_next_walks_the_template_not_the_params:
>   left: Err(OpenScreen)
>  right: Ok(ReturnTarget("/orders/o1/chat"))
> ```
>
> **Mutants M1-M6, planted on the committed fix, quoted, reverted with scoped edits,
> `git status --short` clean after each**: M1 (return the caller's decoded string whole instead of
> the rebuilt template) and M2 (skip re-encoding the captured segment) are BOTH caught by the SAME
> assertion, `safe_next_re_encodes_the_decoded_bypass_bytes` (kept separate from the ordinary
> re-encoding cases so `assert_eq!`'s abort-on-first-failure never masks which case caught it):
> ```text
> an encoded backslash round-trips encoded, never raw
>   left: Ok(ReturnTarget("/jobs/a\\b"))
>  right: Ok(ReturnTarget("/jobs/a%5Cb"))
> ```
> M3 (return the template literal instead of the rebuilt path) reds
> `safe_next_rebuilds_a_param_route_from_its_captured_segments`:
> ```text
>   left: Ok(ReturnTarget("/jobs/:orderId"))
>  right: Ok(ReturnTarget("/jobs/abc"))
> ```
> M4 (walk `params` by ORDER instead of the template — `.last()` instead of by-name lookup) reds
> `safe_next_walks_the_template_not_the_params` — the injected `slug` bleeds into the `:orderId`
> slot:
> ```text
>   left: Ok(ReturnTarget("/orders/chez-test/chat"))
>  right: Ok(ReturnTarget("/orders/o1/chat"))
> ```
> M5 (drop the `requires_auth` check) reds both `safe_next_rejects_an_open_screen` and the
> route/screen property test:
> ```text
>   left: Ok(ReturnTarget("/sign-in"))
>  right: Err(OpenScreen)
> ```
> M6 (beck: delete the `value.starts_with(':')` guard in `rebuild_from_template` — the placeholder
> guard had never been seen red under its own mutant before this run) reds
> `safe_next_rejects_the_template_placeholder`:
> ```text
>   left: Ok(ReturnTarget("/jobs/%3AorderId"))
>  right: Err(Placeholder)
> ```
>
> **Pre-existing, documented not fixed** (ux): a decoded `?` inside a `?next=` candidate still
> silently splits the query off (`safe_next`'s single decode+split, unchanged by this card) —
> `/jobs/a%3Fb` resolves to `/jobs/a`, quietly dropping whatever followed the `%3F`. This is a
> lossy SUBSTITUTION of the destination, not a rejection — pre-existing behaviour this card
> inherited and pinned (`safe_next_boundaries_after_decode`), not introduced by it.
>
> **Two consumption boundaries discard the rejection reason** (observability — where a future
> beacon hooks): `sign_in_return.rs::resolve_return_target` collapses `safe_next(...).ok()` to
> `ReturnTarget::root()`, and `next_param.rs::same_tab_next_override` collapses it to `None` — both
> call sites already had this `.ok()`-shaped seam before this card (the OLD `Option`-returning
> `safe_next` offered nothing else), so the closed `NextRejection` set exists at the ONE place
> (`safe_next` itself) that can distinguish outcomes; wiring a counter at either consumption point
> is future work, not this card's.
>
> **OUT OF SCOPE**, named explicitly: the SSR 302 leg composes no `?next=` at all
> (`crates/server/src/hosts.rs:199-205`, graphql-architect) — filed as
> [#916](https://github.com/TheCaptainCompany/captain-food/issues/916) item 7; the staff chat
> (`order_conversation`) and rider job (`job_detail`) screens have no not-found state of their own,
> so a stale `next` today renders their ordinary empty state with a live compose box (ux) —
> [#947](https://github.com/TheCaptainCompany/captain-food/issues/947); the query half of item 2
> (parsing a `?next=` that itself targets a route WITH a query component) stays a V0 gap; the
> RESERVED `?next=` outcome row for `specs/observability.yaml`, the producer→consumer round-trip pin
> (`bounce.rs`'s `percent_encode_next` against `safe_next`'s rebuild), the vocabulary notes, and the
> third `navigate_away` definition — filed as
> [#949](https://github.com/TheCaptainCompany/captain-food/issues/949) items 1-5.
>
> **Process note** (self-caught, no card defect): implementation began on the branch before it was
> pushed and before the draft PR was opened — the protocol's interlock (push + draft PR BEFORE the
> first line of code) was skipped this run. No shared ref was touched before the branch+PR existed
> together (one commit, `97f15bb7`, still draft, auto-merge never armed), but recorded so the next
> executor treats "branch pushed, draft PR open" as the gate before writing code, not just before
> the first commit.
>
> **No card defects** beyond the process note above (which is a process observation, not a defect
> in the dispatch card's own text) — the design section's exact byte sequences for the re-encoding
> tests (`%20`, `%23`, `%25zz`, `%5C`, `%0D%0A`) all matched what the implementation needed, with no
> correction required.
>
> Lane B. Links: [#916](https://github.com/TheCaptainCompany/captain-food/issues/916),
> [PR #948](https://github.com/TheCaptainCompany/captain-food/pull/948),
> [#947](https://github.com/TheCaptainCompany/captain-food/issues/947),
> [#949](https://github.com/TheCaptainCompany/captain-food/issues/949).

> **2026-09-07 — [#926 "#924 follow-ups (the Red-first gate, round 2): the none form is a prefix
> glob, per-hit is unenforced, the Rust token mirror is unpinned, sharper 0-hit cases, the
> gate-scripts job growth is unmetered"](https://github.com/TheCaptainCompany/captain-food/issues/926)
> item 6, resolved together with [#914 "#910 follow-ups (the Red-first gate): word-bounded tokens,
> RF4 cannot go red for its own reason, an EOF case, an existing-path case, a derived selftest pin,
> the NEW:: fraction metered"](https://github.com/TheCaptainCompany/captain-food/issues/914) item
> 10, draft [PR #945](https://github.com/TheCaptainCompany/captain-food/pull/945), Lane B
> (session_01H3AFBVzhSiGXJcFuwKjiMQ, `926-consults-exempt-from-rule-1`).** **The defect**: Rule 1's
> red-first card step (ADR-20260906-024838) gates every write-capable `Agent` call by Lane D of
> `register-check.sh`, and `architect` is write-capable — so a genuine what-next CONSULT sent to
> `architect` ("read-only" in the prompt, nothing to file or write) still had to carry a resolvable
> `Register check:`/`Red-first:` trail just to pass the gate: a trail written only to satisfy a
> mechanism, never because the call needed one — the exact theatre `Register check: none` was
> refused for being.
>
> **Option space**: (a) no hook change, leave the theatre as the cost of a uniform gate; (b) a
> READ-ONLY TWIN of `architect` for consults, ungated by construction (no `Write`/`Edit`/`Agent`
> tool, so Lane D's own discriminator reads it advisory); (c) strip `Write`/`Edit` from `architect`
> itself, moving its issue filing and proposal writing elsewhere. **Consent, unanimous, option
> (b)** — Consulted: reviewer (persona-text drift between the twin and `architect.md` is accepted
> and stated, not gated); beck (a derived number needs its antecedent — caught the header comment's
> bare "fourteen agents", fixed before D3's commit: "the other agents, `architect-consult`
> included"); farley (workflow.md :1047-1055 REWRITTEN, not appended — the finding kept as one
> sentence of history); holub (no third what-next voice — copy `architect.md`'s body, change only
> the sentences that speak of filing or writing; `Bash` on the twin is for READING only — `gh issue
> list`, `git log`, `make validate`, a GET — never `gh issue create`, `git commit`, `git push` or a
> heredoc that writes a file, added to the twin's Hard Boundaries in D3's first commit).
>
> **Landed**: NEW `.claude/agents/architect-consult.md` (`tools: Read, Grep, Glob, Bash`, no
> `Agent`), persona body identical to `architect.md`'s except MODE 2 (file/propose) redirected to
> `architect`, the "operations role" sentence and the Hard Boundaries Priority/filing bullets
> reworded to "recommend, never write". `architect.md` itself untouched (LD1 still pins it
> write-capable). NEW selftest case LD4 (live-roster wiring,
> `.claude/hooks/register-check-selftest.sh`) — a trail-less consult to `architect-consult` must
> ALLOW as `agent-advisory`. **D1 red** (before the file existed):
> ```text
> register-check: this dispatch carries no `Register check:` trail, and it is GATED because no
> `.claude/agents/architect-consult.md` declares this agent, so its tool set cannot be read (an
> undeclared agent — `general-purpose` is the live case — holds the full set, including
> Write/Edit).
> ```
> **D2 mutant red** (`, Write` appended to the twin's `tools:` line, reverted immediately after —
> `diff` against a pre-mutation backup confirmed byte-identical, `git status --short` showed only
> intended files):
> ```text
> register-check: this dispatch carries no `Register check:` trail, and it is GATED because
> `architect-consult` is write-capable (`tools: Read, Grep, Glob, Bash, Write`, grants `Write`) —
> this call can produce a diff.
> ```
> Both reds are quoted verbatim in LD4's own comment (durable), not only here. Hook header
> comments (`register-check.sh` :52-70, :609-612) name the variant; no ARM change.
>
> **Docs rewritten**: `docs/claude/sessions/workflow.md` :1047-1055 — the 2026-09-04 finding ("put
> the trail on every card, whoever receives it") kept as one sentence of history; the rule now: a
> what-next consult routes to `architect-consult`, which Lane D never gates, no trail line needed;
> the `Register check:`/`Red-first:` trail belongs on write-capable dispatches only
> (`architect`/`executor`/`generator`); a consult sent to `architect` carrying an invented trail is
> the theatre this variant ends. The card template (~:860, "Gated at dispatch by Lane D…") gets one
> line naming the consult target and the exemption.
>
> **(c) QUEUED, not decided** — new OPEN row
> [`docs/decisions/ARCHITECT-WRITE-SCOPE.yaml`](../decisions/ARCHITECT-WRITE-SCOPE.yaml), owner
> `team`: should `architect` itself lose `Write`/`Edit` too? Antecedents:
> [PROP-20260726-193000](../proposals/PROP-20260726-193000-continuous-development-loop.md) ("Two
> agents") originally landed `architect` as a READ-ONLY dispatcher before Write/Edit were later
> granted; [ADR-20260810-011500](../adr/ADR-20260810-011500-team-ownership-sessions-start-autonomously-coordinator-never-authors.md)
> §1 assigns every phase of the diff to the executor, never the coordinator or the architect — the
> same shape option (c) would extend to filing/proposal-writing;
> [ADR-20260831-141500](../adr/ADR-20260831-141500-the-coordinator-gets-the-register-check-gate-on-its-committing-surface.md)
> :200-205 states, as an open unknown, whether Lane D even fires on `architect`'s own NESTED
> `Agent` calls. Tracked as item 3 of [#946 "#945 follow-ups (architect-consult): an automated LD4
> mutant case, the twin drift gate, option (c) for the architect tool
> line"](https://github.com/TheCaptainCompany/captain-food/issues/946).
>
> **Checkpoint non-blocking notes, both filed on #946**: an automated LD4b mutant case driven via
> `REGISTER_CHECK_AGENTS` on a temp copy of the agents dir (rather than the manual
> mutate-run-revert this run did by hand) is a follow-up for the coordinator to file (item 1); the
> twin's ~260 shared lines with `architect.md` carry no drift gate today — revisit when
> `architect.md` changes twice without the twin following (item 2).
>
> **Waste named**: nine consult cards this lane carried a negative trail only to pass the gate
> before this variant existed — the coordinator's own count, UNVERIFIED input (not independently
> re-counted here).
>
> **No card defects** beyond the one caught and fixed inside this same run: the bare "fourteen
> agents" derived number in the header comment (no antecedent stated), caught by beck at the
> checkpoint per ADR-20260817-105845 and rewritten in D3's first commit before it reached `main`.
>
> **Architect's dated re-ranking** (ADR-20260810-215503): [#943 "#942 follow-ups (silent-refresh
> latch): the push socket credential, /auth/refresh observability, a server pin for
> 401-before-dispatch, the handwritten.rs comment, a DPIA session
> row"](https://github.com/TheCaptainCompany/captain-food/issues/943) item 1 re-bucketed `Urgent`
> → `Medium` after the `ux` lens corrected its own finding: the restaurant queue never receives new
> orders over the push socket at all (no `refetch`, no restaurant-scoped push), so item 1 is not
> the socket-credential gap it first read as; the actual first user-visible chunk is filed as
> [#944 "Restaurant queue: no restaurant-scoped new-order push and no refetch —
> orders.byRestaurant is read once at
> mount"](https://github.com/TheCaptainCompany/captain-food/issues/944), sequenced to land after
> [PR #933](https://github.com/TheCaptainCompany/captain-food/pull/933) merges. This reverses the
> previously stated order and is recorded here per the method clause. **This #926/#914 chunk is
> DARK** (no user-visible change): every user-visible candidate in the current queue needs a
> screens/api/observability edit that sits behind the #933 lock, so the lane worked gate tooling
> instead.
>
> Lane B. Links: [#926](https://github.com/TheCaptainCompany/captain-food/issues/926),
> [#914](https://github.com/TheCaptainCompany/captain-food/issues/914),
> [#945](https://github.com/TheCaptainCompany/captain-food/pull/945),
> [#946](https://github.com/TheCaptainCompany/captain-food/issues/946),
> [#943](https://github.com/TheCaptainCompany/captain-food/issues/943),
> [#944](https://github.com/TheCaptainCompany/captain-food/issues/944).

> **2026-09-07 — [#916 "#904 follow-ups (silent refresh + ?next=): latch the one-shot on failure
> not use, `:param` routes in safe_next, wasm client timeouts, the same-tab captured `next`,
> `auth_refresh_total{outcome}` server-side"](https://github.com/TheCaptainCompany/captain-food/issues/916)
> item 1 only, draft [PR #942](https://github.com/TheCaptainCompany/captain-food/pull/942), Lane B
> (session_01H3AFBVzhSiGXJcFuwKjiMQ, `916-refresh-re-arms-on-success`).** `RefreshingTransport`'s
> 401-refresh-and-reissue guard was a once-per-load budget: after ANY bare 401 it stayed spent for
> the rest of the page, even once a refresh had actually rotated the session and a reissue had
> succeeded. **Design**: a bare 401 arrives → if the page is already latched, return it untouched →
> else latch (the `swap(true, ...)` also stops a concurrent mutation-dispatcher 401 from launching a
> second, overlapping rotation) → call the refresher → on refresh failure, return the ORIGINAL 401,
> stay latched → on refresh success, reissue the SAME request once → clear the latch STRICTLY AFTER
> inspecting the reissue's result, and only if it came back `Ok`. Any `Err` on the reissue (401,
> 403, network, malformed) leaves the page latched. **Lens split it resolved** (ADR-20260904-013834,
> the team decides): reviewer read "re-arm on any non-401 reissue result"; graphql-architect read
> "never re-arm on a network or malformed reissue error either" — the TIGHTER option (never re-arm
> on anything but `Ok`) was taken.
>
> **Pin replacement**: `three_401_reads_refresh_exactly_once` is GONE — name and comment, not
> repurposed (beck) — because its assertion (a per-load refresh count of 1) was true for the wrong
> reason under the new design (an unconditional one-shot, not "the reissue didn't come back Ok").
> Its surviving property is pin (a), `a_refresh_whose_reissue_still_401s_latches_the_page`: a
> refresh whose reissue still 401s leaves the page latched, GREEN at base as a regression pin (not
> red-first) — script `[Err 401, Err 401, Err 401]`, refresher always `Ok`, exactly three entries so
> a per-request or always-re-arm mutant over-consumes it and `FakeTransport` panics rather than
> merely miscounting. A second arm (`query { a_net }` / `query { b_net }`, its own documents so a
> red names which arm caught it) covers a non-401 reissue error (`Network`), since ANY `Err` must
> leave the page latched, not just a 401 one. **New red-first test**
> `a_successful_refresh_re_arms_for_the_next_expiry` failed at base with the verbatim panic:
> ```text
> read 2: Err(Status { status: 401 }) is not Ok(...)
> ```
> because the old flag never reset after a successful reissue. **Card defect, attribution card**: an
> earlier architect consult claimed the OLD `three_401_reads_refresh_exactly_once` test would stay
> GREEN if simply run against the D2 fix; tracing it through shows this is wrong — under the fix, its
> first read's successful reissue clears the latch, so its SECOND read re-arms a genuine second
> refresh instead of returning untouched, and its THIRD read (`query { c }`) hits an exhausted
> four-entry script and panics `FakeTransport: unscripted call: query { c }` before the test's own
> count assertion is ever reached — the exact outcome the dispatch card's own "beck's correction"
> section had already predicted, which is why the test is replaced rather than edited, not why it
> "still passes".
>
> **Language change** (evans): the concept is a memory of a failure, not a budget that gets spent.
> `graphql.rs`: the struct field `used` → `latched` (and the `new` parameter, local uses); the
> `mod tests` section banner "the one-shot 401-refresh decorator" → "the 401-refresh latch
> decorator". `renderer.rs`/`interact.rs`: the Arc-carrying identifier `refresh_used` →
> `refresh_latched` (renderer.rs's two locals and the `interact::install` parameter + its one use),
> dropping the "not a once-per-load budget" disclaimer the rename makes unnecessary.
> `handwritten.rs`'s own local `refresh_used` and its stale "one-shot-refresh budget" comment are
> **left untouched** — locked behind PR #933 (#943 item 5).
>
> **Mutants planted on the committed fix, quoted in the tests' own doc comments (not just the
> hand-back, since a hand-back is not durable), reverted, `git status --short` empty after each**:
> M1 (never re-arm, restore the unconditional latch) reds the new test with
> `read 2: Err(Status { status: 401 }) is not Ok(...)`; M2 (re-arm regardless of, or before, the
> reissue's result) reds pin (a)'s primary arm with
> `FakeTransport: unscripted call: query { b }`; M3 (re-arm on any `Err` of the reissue, inverted
> `is_ok`/`is_err`) is ALSO caught by pin (a)'s primary arm, with the same
> `query { b }` panic — the dispatch card had guessed this would need the second (non-401) arm, but
> tracing it shows the primary arm's own reissue-still-401 case already exercises "did the latch get
> wrongly cleared", so M3 never reaches the second arm; M4 (re-arm unless the reissue is
> specifically a bare 401 — a sneaky mutant that PASSES the primary arm, since a reissue-401
> correctly stays latched under it too) reds the second arm specifically with
> `FakeTransport: unscripted call: query { b_net }` — the reason that arm needed its own,
> distinguishable documents.
>
> **OUT OF SCOPE**, named explicitly and filed on
> [#943 "#942 follow-ups (silent-refresh latch): the push socket credential, /auth/refresh
> observability, a server pin for 401-before-dispatch, the handwritten.rs comment, a DPIA session
> row"](https://github.com/TheCaptainCompany/captain-food/issues/943): the push socket re-sends its
> captured `auth` unchanged through backoff and never through `RefreshingTransport`
> (`subscriptions.rs` ~:520-523) — an expired credential on that handshake silences the queue while
> the page looks healthy (item 1); `/auth/refresh` is unobserved — no span, no counter, no
> correlation id (item 2, pending item 5 of #916 itself); the transient-network latch-until-reload
> behaviour is correct per pin (a)'s second arm, not a gap, but stays a silent degraded mode until
> item 2 lands a signal (item 3); young's server pin that a 401 always precedes any dispatch, so a
> reissue is a first delivery never a duplicate command — pinned by reading only today (item 4); the
> stale `handwritten.rs` comment behind the #933 lock (item 5); legal's completeness note (never
> clearance) that after a re-arm the effective unattended back-office session becomes the refresh
> cookie's full 30 days (`auth_routes.rs` ~:196) — a bounded-session choice to record under #194's
> DPIA, not this diff (item 6); ux's pre-existing false signifier, a `requires_auth` screen with no
> `unauthenticated_route` showing a 401 as "Network problem — tap to retry" (item 7); roster note —
> the loop-budget guard's `start` refused (exit 3, INTEGRITY) at this run's first tool call because
> the coordinator's own timer for the same run id was already open; resolved per the guard's message
> (`stop` then `start`), which closed the coordinator's timer and put two ledger files on this
> branch — **attribution: card**, since the dispatch card never stated the timer was the
> coordinator's, and the guard's exit-3 text reads identically for "a rival session's timer" and
> "your own coordinator's still-open one" (item 8); farley's stale incremental `web` test binary
> reporting the new tests red on this branch after a checkout (cargo's mtime fingerprint judged it
> fresh; `cargo clean -p web` fixed it, cost: one full rebuild) — one line for
> `docs/claude/sessions.md` if it recurs a second time (item 9).
>
> **UNVERIFIED input, unchanged from the checkpoint**: the access cookie's `Max-Age` is the
> identity provider's `expiresIn`, 3600 seconds when unreported
> (`crates/server/src/auth_routes.rs:186`) — the provider's real value is unreadable from here, so
> any 1h/2h-versus-peak-window timeline stays UNVERIFIED input and is not narrated as fact anywhere
> in this record.
>
> **No `docs/claude/sessions/gates.md` line**: both operational findings this run surfaced (the
> loop-budget guard's ambiguous exit-3 message, and the stale incremental test binary) are already
> filed as follow-up items on #943 (items 8 and 9) rather than sessions/gates material — item 9's
> own text conditions a sessions.md line on the defect recurring a SECOND time, which it has not
> (yet).
>
> Lane B. Links: [#916](https://github.com/TheCaptainCompany/captain-food/issues/916),
> [#942](https://github.com/TheCaptainCompany/captain-food/pull/942),
> [#943](https://github.com/TheCaptainCompany/captain-food/issues/943).

> **2026-09-07 — [#834 "Four hard-coded English strings on the checkout pay step, and two declared
> keys with no runtime consumer"](https://github.com/TheCaptainCompany/captain-food/issues/834),
> PARTIAL slice, draft [PR #939](https://github.com/TheCaptainCompany/captain-food/pull/939), Lane B
> (session_01H3AFBVzhSiGXJcFuwKjiMQ, `834-checkout-title-from-the-catalog`).** The checkout screen's
> `<h1>` (`back_button_header`) and the SSR `<title>` now resolve `checkout.title`
> (`restaurant_frontoffice.translations.yaml:92`, en "Checkout", fr "Paiement") from the locale
> already normalized at `render_checkout_html:586` — never the caller's raw argument — instead of
> the hard-coded English literal `"Checkout"`. The " - Captain.Food" brand suffix stays a literal,
> exact spelling: four other `page_html` callers (tracking.rs, sign_in_return.rs,
> admin_sign_in_return.rs, invitation_accept.rs) spell the suffix identically; renderer.rs carries
> the bare brand with no suffix (corrected at the confirmation round — the round-1/2 hand-backs said
> "five", which was wrong). The brand is a proper noun, locale-invariant. **NO entry was written in
> `specs/translations.code_refs.yaml`** —
> ADR-20260725-013315's over-declaration precedent decided a lens split (reviewer, evans, farley,
> holub, graphql, vernon, beck said no gate needs it — the screen's own `$ref` already marks
> `checkout.title` used and an entry would re-create the over-declaration that ADR records; ux,
> business, dba, observability, young leaned yes but named no gate it changes) — the code_refs
> registry is for keys NOT referenced from any screen, and `checkout.title` is.
>
> **Reds, base**: four tests, all RED on the fr arm at 3806bb8 (en arms are regression guards only —
> the literal being replaced already equalled the en catalog value, so an en arm can never be red at
> this base): the heading test, the tab-title test, a wrong-key test (folded away in round 2, see
> below), and a region-tagged-locale test. **Mutants, planted/quoted/reverted**: (1) restore the
> `"Checkout"` literal in the `<h1>` — reds the heading test (and, before the fold, the wrong-key and
> region tests too — **the hand-back's original "reds only its own test" claims were wrong; the
> reviewer measured the h1 mutant actually reds THREE tests and the title mutant reds TWO**, since
> the wrong-key and region tests both duplicated the heading/title assertions); (2) restore the
> literal page title — reds the tab-title test (and the region test); (3) resolve `checkout.contact`
> instead of `checkout.title` — reds both locale arms, confirmed by planting it on the h1 site and
> observing the fr AND en arms go red INDEPENDENTLY (not "by construction"); (4) use the raw,
> un-normalized `lang` argument for the `<html lang>` attribute instead of the normalized one — reds
> the region test's `fr-FR` case (`<html lang="fr-FR">` vs expected `"fr"`). **The card's own region
> mutant ("resolve the title from the raw lang argument") was UNKILLABLE as literally stated**:
> `i18n::resolve` normalizes its own `locale` argument internally, so `resolve("checkout.title",
> "fr-FR")` already reduces to `"fr"` and matches regardless of which argument (raw or normalized)
> is passed — the actually-plantable disagreement is either the `<html lang>` attribute (mutant 4
> above) or an UNSUPPORTED locale, added as a second case (`"de"`) in round 2: `DEFAULT_LOCALE =
> "fr"` (`render_checkout_html`'s own fallback) and `FALLBACK_LOCALE = "en"` (`i18n::resolve`'s
> fallback for an unrecognized locale) are DIFFERENT constants, so resolving the title from the raw
> `"de"` argument renders `<title>Checkout - Captain.Food</title>` over a `<h1>Paiement</h1>` and
> `<html lang="fr">` — planted, confirmed red (`left: "<title>Checkout - Captain.Food</title>"
> right: "<title>Paiement - Captain.Food</title>"`), reverted.
>
> **Two decisions changed from the dispatch card, both reviewer-caught at the checkpoint**: (1) the
> card specified comparing against `i18n::resolve("checkout.title", lang)` directly; the tests
> instead assert LITERAL expected copy ("Paiement", "Checkout") — stricter, and avoids a
> resolve-versus-resolve tautology where a mutant that changes the call site AND happens to still
> resolve consistently would not be caught. (2) The card kept the `[checkout.title` fail-visible
> marker assertion "as a diagnostic"; it was never added — superseded by the exact-literal
> `assert_eq!`s, which already fail loudly (and print the marker verbatim in the slice) if the key
> ever went unresolved.
>
> **ux's reading-order sentence, verbatim**: "the customer's eye lands on French in the tab and the
> header, then hits English two elements later on the money — the tab and the heading are now the
> two most French things on a screen whose total still reads English: a visible seam, not a fixed
> screen." **legal's sentence**: "this slice remediates a loi 94-665 (Toubon) art. 2 exposure on a
> heading and a tab title and does not close it — nobody should read PR #939 as 'the checkout is
> French now'." Three counsel questions and a WCAG line appended to the standing packet on
> `docs/adr/ADR-20260904-152807-*.md` (addendum, 2026-09-07, this change).
>
> **Residue**: `:388` (`format!("{} items - {}", cart_line_count, formatted_total)`, the cart
> summary) and `:473` (`" from "` between the summary and the restaurant name) stay English pending
> PR #933's merge and
> the specs lock lifting; `checkout.processing` is a DECLARED key with no runtime consumer yet (a
> submit-in-flight signal this SSR tree lacks). `tracking.rs:508` carries the SAME hard-coded-title
> defect (`page_html("Your order - Captain.Food", lang, &body)`) on the order-tracking screen — a
> HIGHER-stakes surface (post-purchase, the customer is waiting on their order) — flagged as the
> next-chunk candidate on [#941 "#939 follow-ups (checkout
> copy)"](https://github.com/TheCaptainCompany/captain-food/issues/941) item 1, not fixed here.
>
> **Card defects**: the briefing's framing that "both [locale arms] are red" at the base was wrong —
> the en arms can never be red at this base (the literal being replaced already equals the en
> catalog value); the `[checkout.title` marker instruction was superseded before it was ever
> exercised; the code_refs framing undersold that the decision was ADR-20260725-013315's precedent
> deciding a lens split, not an open question; the region-test mutant as literally described in the
> card was unkillable (see above); and the round-1 hand-back's mutant-scope claims ("reds only its
> own test") were wrong, corrected by the reviewer's full-suite measurement.
>
> **Roster notes**: vernon's region-agreement test was reshaped (the "de" case added, the fr-FR-only
> design changed) at the round-2 checkpoint without vernon on that checkpoint's roster — a
> depth-of-the-invited-lens question, not a roster-width miss, since vernon's own briefing concern
> (locale agreement) is exactly what got strengthened. Thirteen lenses each read a pinned line number
> at the briefing and none rendered the whole French checkout page end to end (ux) — no lens caught
> that unsupported-locale fallback had no owner until beck's round-2 pass (beck). **A lens planted a
> mutant through a HARDLINKED scratch copy of `checkout.rs`** during the checkpoint — a hardlink
> shares the inode with the real file, so editing the "scratch" copy edited the file every build and
> test in the shared tree was reading, leaving a planted `raw_lang` mutant visible in the shared tree
> for a period before it was caught and reverted. **Scratch copies must be real copies (`cp`, not
> `ln`)** — recorded as `docs/claude/sessions/gates.md` §19l, the one line judged not derivable from
> the code (cost: a false stop-hook report plus a mutant visible in the shared tree during a
> checkpoint another lens was reading).
>
> Lane B. Links: [#834](https://github.com/TheCaptainCompany/captain-food/issues/834),
> [#939](https://github.com/TheCaptainCompany/captain-food/pull/939),
> [#941](https://github.com/TheCaptainCompany/captain-food/issues/941).
