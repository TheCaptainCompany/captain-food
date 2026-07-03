# Captain.Food – Web Client Product Spec (V0)

## 1. Target users and devices

Primary user:
- Customer who wants to order food from independent restaurants in a specific city (Tours for V0).

Devices:
- Mobile phones first (4G, small screens).
- Should also work correctly on desktop.

Constraints:
- Page loads under ~2 seconds on 4G for main flows.
- An account is required to place an order (Uber Eats model). Identification is by
  **phone number via SMS one-time code (OTP) — passwordless**. Low friction, no password to manage.

## 2. Roles

- Anonymous visitor — can browse restaurants and build a cart, but must verify a phone number to check out.
- Authenticated customer — required to place an order; identity (phone OTP) managed by Supabase Auth.

## 3. Main user flows (V0)

### 3.0 Identification (phone OTP, account required)

- As a customer, I want to identify myself with just my phone number, modelled on Uber Eats.
- Identity is delegated to **Supabase Auth** with the phone provider (not hand-rolled).
- Flow (passwordless, single path for sign-up and sign-in):
  1. Customer enters their phone number.
  2. Supabase Auth sends a one-time code by SMS.
  3. Customer enters the code → verified and authenticated.
  - If the phone number is new, a `Customer` is created; otherwise the existing one is reused.
  - No password exists, therefore there is no "lost password" flow.
- Biometric re-authentication (to avoid an SMS on every sign-in):
  - After the first phone verification, the app **recommends enrolling a passkey** — Face ID /
    Touch ID / device biometric, via WebAuthn (Supabase Auth passkeys).
  - A returning customer with a passkey re-authenticates with the biometric → **no SMS sent**.
  - Fallback to phone OTP when no passkey is available (new device, unsupported browser).
  - This cuts SMS cost and friction. Passkey enrolment/sign-in is a provider concern → NOT a
    domain event (same as OTP and sessions).
- V1 (optional): social login (Google / Apple / Facebook), capture an optional email for receipts.
- Gating:
  - Browsing restaurants, viewing catalogs and building a cart do NOT require identification.
  - Checkout (3.5) requires a verified phone; an anonymous visitor is prompted to verify before paying.
- Domain impact:
  - On first verification of a new number, a `CustomerRegistered` domain event creates the
    `Customer` aggregate, linked to the Supabase Auth user id (`authRef`).
  - OTP send/verify, token refresh and sessions are provider concerns and are NOT recorded
    as domain events.
- Dependencies:
  - SMS provider behind Supabase Auth (e.g. Twilio / Vonage / MessageBird) — a per-message cost.
  - Passkeys are a Supabase Auth **beta** feature; WebAuthn relying party must be configured
    (RP ID = bare domain `captain.food`, allowed origins limited to 5). See the story-map gap
    about per-restaurant subdomains.

### 3.1 Discover restaurants

- As a customer, I want to discover restaurants available in my area.
- UI:
  - Homepage shows a list of restaurants with:
    - Name
    - Category (e.g. sushi, pizza, burgers)
    - Rating (if available)
    - Basic badges (e.g. “Local”, “Eco delivery”)
- Data:
  - `GET restaurants` via GraphQL (read from `read_restaurants_public`).

### 3.2 View a restaurant and its catalog

- As a customer, I want to open a specific restaurant and view its catalog quickly.
- UI:
  - Restaurant page at `https://{slug}.captain.food` or `https://captain.food/r/{slug}`.
  - Show:
    - Restaurant name, description, opening hours.
    - Catalog categories (starters, mains, desserts, drinks).
    - Catalog items with name, description, price, and photo.
- Data:
  - GraphQL query: `restaurant(slug: String!) -> Restaurant`.

### 3.3 Build a cart

- As a customer (even as a guest, before phone verification), I want to add items to my cart,
  adjust quantities, and see the total — with immediate feedback if something is out of stock or
  an option choice is invalid.
- UI:
  - “Add to cart” button on each catalog item.
  - Cart summary panel:
    - List of items (name, options, quantity, unit price, line total).
    - Cart total.
  - Surface per-line validation errors returned by the server (out of stock, illegal option, etc.).
- Data:
  - The cart is a **server-side `Cart` aggregate**, mutated via GraphQL (`addCartLine`,
    `changeCartLineQuantity`, `removeCartLine`) and read via the `cart(id)` query (priced server-side).
  - Cart-building mutations are **public** (no auth): a guest builds a cart before verifying a phone.
    The cart is session-scoped (a client-held `cartId` / cart token); `customerId` is bound at checkout.
  - Keep a small client cache for snappy UI, but the server cart is the source of truth and validates
    each line against the live catalog. (This replaces the earlier localStorage-only, no-server-call
    approach; cart persistence/retention follows the `Cart` stream policy in [database.md](database.md).)

### 3.4 Checkout – contact details

- As a customer, I want to enter my contact details and delivery/pickup info.
- UI:
  - Checkout form:
    - Full name
    - Phone number
    - Email
    - Delivery address (if delivery)
    - Pickup vs delivery toggle
  - Basic validation for email / phone formats.
- Data:
  - On submit, call GraphQL mutation `placeOrder` with:
    - `restaurantId`
    - `cartId` (the OPEN cart whose lines become the order)
    - `customerInfo`
    - `deliveryMode`

### 3.5 Checkout – payment (Stripe, card + Apple Pay)

- As a customer, I want to pay online securely, including with **Apple Pay** for a one-tap,
  low-friction checkout on iOS/Safari.
- Flow:
  - After validating the order, backend creates a Stripe PaymentIntent.
  - Frontend uses **Stripe Elements** — the **Express Checkout Element** (Payment Request Button)
    surfaces **Apple Pay** when the device/browser supports it, falling back to the card Payment
    Element otherwise. A single PaymentIntent backs every method. (Google Pay rides the same element
    and can be switched on as a fast-follow.)
  - On successful payment:
    - Stripe redirects to a “Thank you / order tracking” page.
    - Backend writes `PaymentCaptured` and `OrderPlaced` events.
- Apple Pay is purely a **payment-method/UI concern**: it produces a standard Stripe
  `PaymentMethod`, so it does **not** introduce any new domain event — the `PlaceOrder` saga is
  unchanged (see [commands.yaml](commands.yaml) `PlaceOrder.paymentMethodId`).

- UI:
  - Show payment loading state.
  - Surface the Apple Pay sheet when available; otherwise show the card form.
  - Show error state if payment fails.
- Dependencies / setup:
  - Apple Pay requires a **verified Apple Pay domain** in the Stripe dashboard and serving the
    Apple domain-association file. Per-restaurant subdomains (`{slug}.captain.food`) must be
    covered — same single-origin checkout decision as the passkey gap
    ([ADR-0036](../docs/adr/0036-domain-topology-single-origin-identity.md)). Routing checkout through one origin (`captain.food`) covers both.
  - HTTPS is mandatory (already required).

### 3.6 Order confirmation and tracking

- As a customer, I want to see order status in real-time.
- Statuses:
  - `PLACED` (payment captured, order sent to restaurant)
  - `ACCEPTED`
  - `REJECTED`
  - `PREPARING`
  - `READY`
  - `OUT_FOR_DELIVERY` (optional, if we have delivery integration)
  - `DELIVERED`
  - `CANCELLED`

- UI:
  - Order confirmation page shows:
    - Order ID
    - Summary (items, price)
    - Current status
    - Timeline of status changes (optional)
  - Polling or GraphQL subscription (if available) to update status.

### 3.7 Restaurant onboarding entry point

- On the customer app, in the footer or menu:
  - Link: “Become a restaurant partner”.
  - Redirects to `https://restos.captain.food/onboarding`.
- For V0, that page can be:
  - Simple form: restaurant name, contact, current systems used, approx. revenue.
  - Or just a typeform/external form as a placeholder.

## 4. Non-functional requirements

- Performance:
  - Lighthouse score on mobile > 90 for Performance and Best Practices if possible.
  - Initial HTML rendered on server (Next.js SSR/SSG).

- Accessibility:
  - Basic a11y (labels, focus states, semantic HTML).

- Offline (optional V0, planned V1):
  - We may later add a PWA service worker to allow viewing the catalog offline (already described in story maps). 

## 5. Tech constraints

- Next.js App Router.
- Tailwind CSS for styling.
- TypeScript only.
- All calls to backend via GraphQL client (e.g. urql, Apollo, or a simple fetch wrapper).