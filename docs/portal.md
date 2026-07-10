# Portal reference

Field notes on the Town of Jupiter utility portal, for anyone maintaining the
scrapers. Everything here is observable from the public, unauthenticated pages.

## Stack fingerprint

- **Application:** Harris **eCARE** utility billing (modules named
  `eCAREPayNow`, `eCARePayNow`, `eCARePayNowC`, `eCAReDefault`, `eCAReIcons`).
- **Platform:** DotNetNuke (DNN) — ASP.NET WebForms. Menu via `DDRMenu`.
- **Client libs:** Telerik.Web.UI `Version=2013.2.717.35`.
- **TLS:** TLS 1.2 only; CBC ciphers only (`ECDHE-RSA-AES256-SHA384` and
  friends). No GCM, no TLS 1.3. → use native-tls (see `CLAUDE.md`).

## Pages

| Path | Auth | Purpose |
| --- | --- | --- |
| `/Home.aspx` | public / summary when logged in | Landing; account summary post-login |
| `/Login.aspx` | public | DNN forms login |
| `/Register.aspx` | public | Register (needs 7-digit customer + 6-digit account #) |
| `/ForgotUsername.aspx` | public | Username recovery by email |
| `/SecurePasswordRecovery.aspx` | public | Password recovery |
| `/ContactUs.aspx` | public | Contact info |
| `/OnlinePayment.aspx` | public | One-time payment lookup (customer + account) |
| `/BillingHistory.aspx` | auth | Statements |
| `/UsageHistory.aspx` | auth | Metered consumption |
| `/TransactionHistory.aspx` | auth | Ledger |
| `/UserProfile.aspx` | auth | Account holder profile |

Unauthenticated requests to the `auth` pages 302 to
`/Login/tabid/400/Default.aspx?returnurl=…`; nonexistent pages 302 to
`/ErrorPages/404.htm`. That difference is how the endpoint map above was
confirmed.

## Key form controls

DNN prefixes are volatile; match on the suffix.

**Login (`/Login.aspx`)**
- username: `…$Login_DNN$txtUsername`
- password: `…$Login_DNN$txtPassword`
- submit:   `…$Login_DNN$cmdLogin` (posted as `__EVENTTARGET`)
- plus `__VIEWSTATE`, `__VIEWSTATEGENERATOR`, `__EVENTVALIDATION`, `__dnnVariable`

**One-time payment (`/OnlinePayment.aspx`)**
- customer #: `…$PayNow$txtCust`
- account  #: `…$PayNow$txtAcct`
- submit:    `…$PayNow$GoButton`
- On success the page opens a hosted payment window via `window.open(...)`; the
  page also carries a static "turn off your pop-up blocker" instruction, which
  is **not** a per-request status message (the scraper filters it out).

**Register (`/Register.aspx`)**
- `…$userForm$Customer_Number$Customer_Number_Control`
- account number field (labeled "Account Number", leading/trailing zeros)
- `…$Email$Email_TextBox`, `…$Password$Password_TextBox`, confirmations

## Scraping strategy

Data pages render HTML `<table>`s whose exact columns vary. Rather than pin
selectors, `scrape::extract_tables` reads all tables and per-page parsers map
columns by header keywords:

- **Bills:** date / amount / balance / due date
- **Usage:** period / usage (unit sniffed from the header: gallons, kgal, CCF…) /
  days / average
- **Transactions:** date / description / amount (credits parenthesized) / balance
- **Profile:** DNN label→value pairs, matched by id suffix

Unrecognized columns are preserved verbatim in each record's `extra` map.

## Contact

Town of Jupiter Utilities — customer service **(561) 741-2300**.
Utilities home: <https://www.jupiter.fl.us/water>.
