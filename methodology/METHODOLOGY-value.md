# BLACKOUT: Why It Matters

*Policy methodology, September 2026. This page explains what BLACKOUT shows, why it matters, and how to use it responsibly. For how the data is collected and processed, see the Technical Methodology.*

---

## Bottom line

BLACKOUT is an open-source prototype that brings together a dozen public sources on internet censorship and shutdowns into a single, country-by-country view.

It does not collect new data. Its contribution is to place evidence that normally sits in separate places, held by separate research communities, side by side. With everything in one place, an analyst can see what kind of repression is happening in a country and how strong the evidence for it is.

BLACKOUT is built to strengthen **detection**, the first step in any response to digital repression. It shows what is happening. Decisions about how to respond remain with the people using it.

---

## Two kinds of repression

Governments restrict the internet in two fundamentally different ways.

**Filtering.** The network keeps running, but specific websites, services, tools, or types of traffic are blocked. People can still get online. Circumvention tools, such as VPNs and Tor, can in principle adapt by finding a new route or disguising their traffic.

**Disconnection.** The country is cut off from the global internet. There is no route left to adapt to. Circumvention cannot work around a connection that no longer exists. The only remedies are independent paths such as satellite service or local mesh networks.

A response designed for one does not answer the other. Telling them apart is therefore the first question in any crisis, and no single data source can answer it. The evidence for each looks different:

- **Filtering** shows up in measured blocking of particular sites and services, in sudden shifts in the kinds of traffic a country's networks carry, and in rising use of hidden routes into circumvention networks.
- **Disconnection** shows up in outage detection and in a country effectively disappearing from the internet's map.

BLACKOUT places both kinds of evidence in the same view.

### Example: Iran, winter 2025–26

Iran's response to protests in late 2025 moved through both stages.

**First, filtering.** In late December, Cloudflare observed a newer, encrypted method of loading websites nearly vanish from two of Iran's major mobile and fixed-line carriers. It had carried as much as 40 percent of their web traffic, and within days it had fallen to almost nothing. Websites still loaded through older methods, but some circumvention tools depend on the blocked method and stopped working. The government was not blocking a list of sites. It was blocking a whole way of carrying traffic.

**Then, disconnection.** Networks stay reachable by continuously announcing to the rest of the internet where they can be found. It is the equivalent of keeping a listing in a global directory. On January 8, Iranian networks withdrew those listings for 98.5 percent of the country's address space. By that evening, national traffic had fallen effectively to zero.

**Afterward, a harder internet.** When international access returned, it returned under a stricter model. Instead of blocking named destinations, authorities denied everything that did not appear on an approved list.

These are the kinds of signals BLACKOUT brings together: changes in traffic patterns, routing, detected outages, measured blocking, and circumvention use. Read side by side, they make the shift from filtering to disconnection visible. Read separately, each tells only part of the story.

---

## What BLACKOUT shows

| Question | What BLACKOUT provides |
| --- | --- |
| Is this country being filtered, disconnected, or both? | Outage detection, routing visibility, and measured blocking shown side by side |
| Which circumvention tools are people still using? | Estimated use of the Tor network, including the different kinds of hidden entry points (obfs4, Snowflake, WebTunnel) that people use when Tor itself is blocked |
| Can people reach messaging services? | Availability of WhatsApp, Telegram, Facebook Messenger, and Signal, reported as blocked only on confirmed evidence |
| Can people reach major AI services? | Whether the websites of OpenAI, Claude, DeepSeek, and Hugging Face are reachable from inside the country |
| What kinds of content are being blocked? | Restrictions on news media, political content, human rights material, and other categories over the past six months |
| Is the situation getting worse? | Ninety days of outage history and daily blocking records going back to January 2024 |
| Could a shutdown here be enforced, or circumvented? | Submarine cable landings, domestic exchange points, and satellite internet availability |
| How does this country compare to others? | A Censorship Index covering about 180 countries, with its components shown alongside it |

Three principles apply throughout:

- **Missing data is shown as missing.** A country with no measurements is shown as having no data, never as unrestricted.
- **Findings are cautious by design.** A handful of failed tests can reflect ordinary network trouble, so BLACKOUT reports a block only when the evidence is substantial. Otherwise it reports the result as "likely blocked" or "inconclusive."
- **Every figure is traceable.** Each display names its source, and the Censorship Index always shows the scores it is built from.

---

## Where BLACKOUT fits

Effective circumvention depends on four operations, performed continuously as censors adapt:

1. **Detect** that interference has begun, and what kind it is
2. **Decide** which response is most likely to work
3. **Distribute** fresh access points to users faster than censors can block them
4. **Recover** a working connection

BLACKOUT strengthens the first of these. It makes the external situation in a country visible so that analysts, funders, and operators can recognize what is happening sooner and with more confidence. It does not decide on a response, deliver tools, or restore connections. Those remain the work of the people and systems that act on what it shows.

That role matters for policy. A standing government response to digital repression needs agreed conditions for when to act, such as an independently verified national shutdown or the appearance of a new blocking technique. It also needs a realistic picture of the techniques censors are actually using, so that tools can be tested against them before a crisis. BLACKOUT is designed to inform both.

**Privacy by design.** BLACKOUT draws only on data that established organizations already publish. It collects nothing from users in affected countries and has no accounts or submission mechanism. It does not create a new channel through which people at risk could be observed.

---

## Two kinds of evidence, kept separate

BLACKOUT presents two different kinds of evidence and deliberately does not blend them.

**Expert assessment: the Censorship Index.** This describes a country's overall environment for free expression. It combines two established international assessments: the V-Dem Institute's Freedom of Expression Index and Reporters Without Borders' Press Freedom Index. Freedom of expression is weighted more heavily, roughly five to three. The combined score is BLACKOUT's own construction; the underlying assessments are not. Both component scores are always displayed, so anyone who prefers a different weighting can apply one. On every score, a higher number means more freedom.

Alongside the index, BLACKOUT shows three V-Dem measures that focus specifically on the internet: government filtering, government shutdowns, and government censorship effort. These speak directly to state conduct online and are often the most decision-relevant figures in the tool.

**Observed evidence: what the network actually did.** This covers outages, routing, measured blocking, circumvention use, and traffic patterns. It is what happened on the network, not an assessment of the political environment.

The two are kept apart because **the disagreements between them are often the most important finding.** Consider a country whose overall assessment is moderate but where measurements show heavy, recent blocking of messaging apps or circumvention tools. That combination indicates a specific policy has outpaced the general picture. Averaging the two into one number would hide exactly that signal.

---

## What BLACKOUT cannot tell you

- **Who is responsible, or why.** BLACKOUT shows independent signals side by side, but it does not combine them into a finding that a government deliberately imposed a shutdown or blocked a service. The signals can point strongly in one direction. Attribution still requires evidence BLACKOUT does not hold.
- **What is happening this minute.** BLACKOUT refreshes its sources on a regular cycle, every six hours by default, and each source has its own publication delays. It is periodically updated, not real-time.
- **Whether an AI service actually works.** AI reachability reflects whether a provider's websites can be reached, not whether its services or models are usable from inside the country.
- **Whether a circumvention tool works right now.** Tor figures are estimates of how many people are using each tool, not tests of whether the tool is currently reachable.
- **Anything about places no one is measuring.** Network measurement depends on volunteers, and coverage is uneven. Where BLACKOUT reports insufficient data, the question is open. It does not mean the answer is favorable.
- **What will happen next.** BLACKOUT reports what has been observed. It makes no forecasts.

**Limits of the Censorship Index.** The index is built from annual, expert-based assessments and describes a country's general environment, not recent events. Some countries are covered by only one of the two assessments; BLACKOUT shows how many sources each score rests on. Countries may be scored from different years, and each year is displayed. The weighting is a reasoned judgment, not a statistically derived one. The press freedom figures come from an edition that predates Reporters Without Borders' 2022 change in methodology. The three internet-specific V-Dem measures are fixed at their 2025 edition.

---

## Using BLACKOUT responsibly

- **Look for agreement.** The strongest findings come from independent signals pointing the same way. For example, a detected outage, a collapse in routing visibility, and a spike in circumvention use in the same country at the same time. Any one of these alone is a lead, not a conclusion.
- **Treat divergence as a finding.** When the expert assessment and the observed evidence disagree, ask why.
- **Check coverage before comparing countries.** Two countries are not equally well observed just because both appear on the map.
- **Cite the original source.** Figures from V-Dem, Reporters Without Borders, OONI, or any other contributor should be cited to that organization. Cite BLACKOUT for the Censorship Index and for the combined view it provides.

---

## Sources

| Source | What it contributes | How often the source updates |
| --- | --- | --- |
| OONI (Open Observatory of Network Interference) | Blocking of websites, AI services, circumvention tools, and messaging apps; blocked content categories; blocking history | Continuously |
| IODA (Georgia Tech) | National internet outages | Continuously |
| Tor Metrics (Tor Project) | Estimated circumvention use, including by type of hidden entry point | Daily |
| Cloudflare Radar | Traffic patterns; confirmation of outages | Continuously |
| RIPE NCC (RIPEstat) | Routing visibility: whether a country remains reachable on the internet's map | Continuously |
| Internet Society Pulse | Internet Resilience Index | Periodically |
| V-Dem Institute *(via Our World in Data)* | Freedom of Expression Index: Censorship Index component | Annually |
| Reporters Without Borders *(via Our World in Data)* | Press Freedom Index: Censorship Index component | Annually |
| V-Dem Institute | Internet filtering, shutdown, and censorship measures | Fixed at 2025 edition |
| PeeringDB | Domestic internet exchange points | Periodic reference data |
| TeleGeography | Submarine cable routes and landing points | Periodic reference data |
| CelesTrak / SatNOGS | Satellite positions | Continuously |
| Compiled from public sources | Satellite internet availability by country | Maintained by hand; sources cited |

---

*BLACKOUT is open source. Its value lies not in measuring anything new, but in making different kinds of evidence readable together, for nearly every country, without hiding what any figure is built from.*
