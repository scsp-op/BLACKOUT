# BLACKOUT — Methodology and Analytical Value

*Prepared September 2026. This document describes what BLACKOUT measures, how its
Censorship Index is constructed, and how its findings should and should not be used. A
separate technical methodology document covers data collection, system architecture, and
implementation.*

---

## 1. Overview

BLACKOUT is an open-source analytical tool that presents internet censorship, network
disruption, and information freedom for nearly every country in the world in a single view.

It does not conduct original measurement. It draws on established public sources — network
measurement projects, academic research institutions, routing registries, and press freedom
organisations — and brings their findings together into one frame, on one scale, so they can
be read against each other.

The analytical contribution is that integration. Four different kinds of evidence bear on
whether a population can access information freely, and they are normally held in four
different places by four different communities:

- **Expert assessment** of a country's political and press environment
- **Direct network measurement** of what is actually reachable from inside the country
- **Routing evidence** of whether a country is connected to the global internet at all
- **Physical infrastructure** — the cables, exchange points, and satellite access that
  determine whether a disconnection order can even be carried out

Each answers a different question. Read together, they support conclusions that none
supports alone. BLACKOUT's purpose is to make reading them together practical.

---

## 2. Why this tool, and why now

**Access to artificial intelligence has become a censorship question.** Whether a
population can reach major AI services is now a meaningful measure of its information
environment, and it is a question the established freedom indices were not designed to ask.
BLACKOUT tracks access to leading AI platforms as a primary category of measurement,
alongside the circumvention tools (software such as Tor and Psiphon that people use to get
around blocking) that have long been monitored. This is the tool's most distinctive
contribution.

**Network shutdowns have become routine instruments of governance** rather than exceptional
events. Annual expert assessments, however rigorous, cannot capture a disruption that
begins on a Tuesday and ends on a Thursday. Continuous observation is now a requirement, not
a refinement.

**Physical infrastructure shapes what can be enforced.** Satellite internet availability,
the number of undersea cables reaching a country, and how much of its traffic stays inside
its own borders increasingly determine whether a government's restriction can actually be
carried out. These physical constraints belong in the same picture as the political
assessments, because in practice they limit each other.

### Questions the tool is built to answer

| Question | What BLACKOUT provides |
| --- | --- |
| How restricted is this country overall? | A composite Censorship Index, with its component scores shown alongside it |
| Is access to AI services restricted here? | Direct measurement of reachability for major AI platforms |
| Is this a deliberate restriction or a technical failure? | Three independent kinds of evidence — outage detection, routing data, and network measurement — which can be compared |
| Is the situation deteriorating? | At least ninety days of outage history, and daily blocking records since January 2024 |
| Could a shutdown here even be enforced? | Undersea cable landings, internet exchange points, satellite availability, and demand for circumvention tools |
| How does this country compare globally? | A worldwide ranking on each of the index's two components |

### What the tool does not do

**It does not forecast.** BLACKOUT reports what has been observed. It makes no predictions
and assigns no probabilities to future events.

**It does not attribute.** The tool can establish that access was disrupted. It does not
establish who ordered the disruption, or why. Attribution requires evidence BLACKOUT does
not hold.

**It does not replace its sources.** Every source is named and linked. The tool is a lens
for reading established research, not a substitute for it.

---

## 3. The Censorship Index

### 3.1 Whose index is this?

**The composite score is original to BLACKOUT. The data underlying it is not.**

This distinction is important enough to state directly:

- BLACKOUT does not employ country experts, conduct surveys, or generate original
  assessments of any country's political conditions.
- BLACKOUT does combine two established international indices into a single composite
  score, using a weighting and scaling approach of its own design. That composite exists
  nowhere else.

The accurate description is therefore: **a derived composite index, built by BLACKOUT, from
published third-party assessments.** The underlying data is authoritative and independently
citable. The combination is BLACKOUT's own, and its value is in putting two indices that use
incompatible scales onto one scale, so that they can be compared and mapped together.

### 3.2 What goes into it

| Component | Produced by | What it assesses | Share of score |
| --- | --- | --- | --- |
| Freedom of Expression Index | V-Dem Institute (University of Gothenburg) | Freedom of expression and the availability of alternative sources of information, assessed by country experts | 62.5% |
| Press Freedom Index | Reporters Without Borders (RSF) | Media pluralism and independence, the legislative environment, and the safety of journalists | 37.5% |

Both are drawn from Our World in Data, which republishes these indices in a standardised,
country-by-country form. The tool links to those specific published datasets rather than to
the originating organisations generally, because that is where the figures it displays
actually come from.

Each index covers roughly 175 countries, and together they cover about 180, which is what
allows BLACKOUT to present a worldwide baseline rather than a selected group of countries.
For each country, the most recent available year is used, and that year is displayed
alongside the score.

**Reference years.** Freedom of expression scores are drawn from V-Dem's 2025 assessment.
Press freedom scores are drawn from RSF's index through 2021, the final year of the edition
that preceded RSF's 2022 methodology revision (see §3.6).

**Freedom of expression is weighted more heavily than press freedom** because it addresses
the broader question and reflects more recent assessment.

### 3.3 Putting the two on one scale

The two indices do not share a scale, and — critically — they do not run in the same
direction.

The V-Dem index already treats higher scores as more free. The RSF series, in the edition
used here, does the opposite: 0 is the best possible score and 100 the worst, so a higher
score means a *worse* environment. BLACKOUT reverses the RSF figure before combining it.

This correction is essential rather than cosmetic. Without it, every press freedom
contribution would pull in precisely the wrong direction, and the resulting composite would
rank the freest countries as the most repressive.

Both components are then placed on a common 0–100 scale on which **a higher score means more
freedom**. The same convention applies to every index score in the tool. The world map is
the one exception: it shades by restriction rather than freedom, and labels its scale
accordingly.

Each component is also shown with a descriptive classification:

- **Press freedom** uses RSF's own published classifications for this edition (Good,
  Satisfactory, Problematic, Difficult, and Very serious), applied to the original score.
- **Freedom of expression** is grouped by BLACKOUT into five equal 20-point bands: Very free
  (80 and above), Free (60 to 80), Partly free (40 to 60), Repressed (20 to 40), and Highly
  repressed (below 20). V-Dem does not publish its own classifications for this index.

### 3.4 How the two are combined

The composite is a weighted average of the two components, in a ratio of five to three in
favour of freedom of expression (62.5% and 37.5%). The resulting freedom score is then
subtracted from 100 to express it as a censorship score, which is what the map shades by.

For example, a country with a freedom of expression score of 40 and a press freedom score
of 60 has a freedom score of (5 × 40 + 3 × 60) ÷ 8 = 47.5, and so a censorship score of
52.5.

Three features of this method matter for interpretation:

**A country missing one component still receives a score.** Where only one of the two
indices covers a country, the composite is calculated from that component alone rather than
leaving the country unscored. This is what gives the index near-global coverage instead of
limiting it to countries both organisations assess.

The trade-off is that a score resting on one source is a weaker estimate than one resting on
two. About 170 countries have both components, and roughly a dozen, mostly small states,
have one. Each country's component scores are shown individually, and the number of
components behind every score is published in the tool's data, so that analysis can be
restricted to fully covered countries where the question demands it.

**A country covered by neither index receives no score at all** and is left unshaded on the
map. The tool does not fill gaps with estimates.

**The composite never obscures its inputs.** The map is shaded by the composite, and
selecting any country shows the component scores behind it, each with its year. A user who
disagrees with BLACKOUT's weighting has everything needed to apply their own. The index is
offered as a transparent starting point for analysis, not as an authority to be accepted on
trust.

### 3.5 Internet-specific censorship indicators

Beneath the headline freedom of expression score, BLACKOUT displays three further V-Dem
indicators that address internet censorship directly:

- **Government internet filtering** — the extent to which a government filters online
  content in practice
- **Freedom from internet shutdowns** — the extent to which a government refrains from
  shutting down internet access
- **Government censorship effort** — the extent of a government's effort to censor online
  information

Consistent with the rest of the tool, all three are oriented so that **a higher score means
less interference**. A country scoring 90 on censorship effort, for example, makes little
effort to censor.

These are frequently the most decision-relevant figures in the tool, because they speak to
state conduct rather than to the general information environment.

**They are deliberately not folded into the composite index.** They are presented separately
so that a user can see where a country's general political environment and its specific
internet conduct diverge — which is often the most informative thing on the screen.

Each of these indicators is published by V-Dem with a stated range of uncertainty rather
than as a single definitive figure. BLACKOUT retains that range and makes it available with
each estimate, rather than presenting a single number as more precise than its source claims
it to be.

**These three indicators are fixed at 2025** and cover about 175 countries. Unlike the
headline scores, which advance to each country's most recent available year, these do not
change until the underlying dataset is deliberately updated. Users should not read them as
evidence of recent change, and should be aware that an internet-specific indicator may not
be from the same year as the headline score displayed above it.

### 3.6 Limitations of the index

These should be understood before the index is cited.

**It is an index built from other indices.** It inherits the limitations of both sources,
including their reliance on expert judgement and their annual rather than continuous
publication. It describes a country's general environment. It does not describe what that
country's network did this week — for that, the observed indicators in §4 are the
appropriate evidence.

**The press freedom component reflects RSF's pre-2022 methodology.** RSF revised its
methodology in 2022, and the standardised series used here covers the years before that
revision, through 2021. RSF's current index is published on a different basis and is not
directly comparable with this series.

**Single-source scores are weaker.** The number of components behind each score is
published for exactly this reason and should be checked before two countries are treated as
equally well-founded.

**The weighting is a reasoned editorial judgement, not a statistical result.** The
five-to-three ratio has not been derived from data or tested against any outside
benchmark. It is stated openly so that it can be questioned, and both components are
published so that an alternative weighting can be applied.

**Countries may be compared across different years.** Each country uses its own most recent
available assessment, and those years may differ. Every year is displayed.

**No network measurement enters the index.** None of the observed data described in §4
contributes to the composite score. This is a deliberate separation, and the reasoning is
set out in §4.

---

## 4. Observed indicators

The Censorship Index describes a country's environment as assessed by experts. The
indicators in this section describe what has actually been observed on the network. They are
kept separate throughout the tool.

The separation is the point. When expert assessment and direct measurement agree, the
finding is strong. When they diverge — a country with a middling index score but heavy
measured blocking, or the reverse — that divergence is itself a significant finding, and it
would be destroyed by averaging the two into a single number.

### 4.1 Access to AI services and circumvention tools

BLACKOUT tracks whether the websites of major AI platforms — OpenAI, Claude, DeepSeek, and
Hugging Face — can be reached from within each country, alongside the established
circumvention tools:

- **Tor, Psiphon, and Snowflake** are tested directly: the test checks whether the tool
  itself can connect.
- **Signal, I2P, and the Tor Project** are tested by whether their websites load. (Signal's
  app is tested directly, as a messaging service, in §4.2.)

These measurements come from OONI, the Open Observatory of Network Interference, whose
volunteers run testing software inside each country. OONI flags each test that shows signs
of interference. BLACKOUT pools every test from the preceding 90 days and classifies each
platform, in each country, by the share of flagged tests and the number of tests behind it:

| Result | Share of tests flagged | Minimum number of tests |
| --- | --- | --- |
| **Blocked** | more than 70% | 11 |
| **Likely blocked** | more than 40% | 6 |
| **Accessible** | less than 10% | 6 |
| **Inconclusive** | anything else, or too few tests | — |

The 90-day window keeps the classification anchored in current conditions rather than in
restrictions lifted long ago, while pooling enough tests to be reliable. Day-to-day change
is captured separately in the blocking history (§4.4). Availability is never inferred from
the absence of evidence.

This conservatism is deliberate: a small number of flagged tests can reflect ordinary network
trouble rather than state action, and reporting that as censorship would be a serious error.

### 4.2 Messaging platform availability

Availability of WhatsApp, Telegram, Facebook Messenger, and Signal is tracked separately.

These platforms are measured by tests designed specifically for each service, which examine
whether the service itself is reachable — not merely whether its public website loads. The
distinction matters in practice: a government can leave a messaging company's website fully
accessible while blocking the service its application depends on, and a simpler test would
record that as unrestricted.

Messaging results are classified using the same thresholds as §4.1, and each is reported
with the number of tests behind it, so that a reader can judge how much weight it will bear.

### 4.3 Categories of censored content

The same body of measurement is also assessed by subject matter, showing which categories of
content — news media, political criticism, human rights material, and others — are
restricted in each country over the preceding six months.

Category-level assessment uses lower thresholds than single-platform assessment, because
a category covers hundreds of individual sites, most of which usually remain reachable even
where a subject is heavily targeted. Applying single-platform thresholds here would make
almost every country appear unrestricted. A category needs at least 100 tests to be
assessed. It counts as **heavily censored** when 20% or more of them are flagged, and as
**partially censored** at 5% or more.

### 4.4 Blocking history

Daily records since January 2024 track the share of tests showing interference for each AI
platform and circumvention tool, in every country where measurements exist. This turns a
point-in-time assessment into a trend, which is usually the more useful form for policy
work.

### 4.5 Internet outages

Country-level outage events are drawn from IODA (Internet Outage Detection and Analysis), a
research project run by Georgia Tech's Internet Intelligence Lab and originally developed at
CAIDA, University of California San Diego. IODA detects national disruptions using four
independent signals:

- **Routing data**: whether the country's networks are still announcing themselves to the
  rest of the internet (see §4.6)
- **Active probing**: whether devices in the country respond when contacted from outside
- **Background traffic**: the stray, unrequested traffic that every connected network
  gives off. When it stops, the network has probably gone dark.
- **Google's Transparency Report**: how much traffic reaches Google's services from the
  country

Events are collected over a rolling 90-day window and retained, building a continuous
outage history.

Each event records when it began, how long it lasted, how severe it was, and which signal
detected it. That last detail carries analytical weight, because the signals fail in
different ways. An outage visible in routing data but not in traffic measurement means
something different from the reverse.

Severity is expressed using IODA's own score, which is specific to each signal. BLACKOUT
groups these scores into Severe, Major, and Minor for readability.

### 4.6 Routing visibility

To be reachable, a network has to announce itself to the rest of the internet, much as a
phone number has to be listed to be called. Each country has a set of networks registered
to it: those run by its internet providers, companies, universities, and government. Using
data from the RIPE Network Coordination Centre, BLACKOUT tracks what share of those
registered networks is currently announced, and therefore visible, on the global internet.

This share is normally well below 100%, because many registered networks are unused or used
only internally. The level on its own therefore says little. **What matters is a sudden drop
against the country's own normal level.** When a government orders networks to withdraw
their announcements, the share falls sharply while the number registered stays the same.

This is one of the strongest available indicators of a deliberate, network-level shutdown.
Because a major power failure or cable cut can also take networks offline, it is read
alongside the outage and measurement evidence (see §6). The figure is refreshed hourly.

### 4.7 Circumvention demand

Usage of the Tor network is tracked by country, using the Tor Project's own estimates. The tool
counts both direct connections and connections through **bridges**: unlisted entry points,
some disguised as ordinary traffic, that people use where Tor itself is blocked.

This is best read as a **demand signal**. A sharp rise in bridge use in a given country is
evidence that ordinary access is being interfered with, whether or not any OONI volunteer
happened to be testing there at the time. It is a useful corrective in countries where
direct measurement coverage is thin.

### 4.8 Web protocol share

Using data from Cloudflare, BLACKOUT tracks what share of each country's web traffic uses
each version of HTTP, the protocol that carries web pages. The one to watch is the newest
version, **HTTP/3**. Some censorship systems find it harder to inspect and respond by blocking
it outright, which also disrupts circumvention tools that rely on it.

A sudden fall in a country's HTTP/3 share can therefore be an early sign of new filtering,
sometimes before any blocking is directly observed. The figures reflect traffic served
through Cloudflare's network, one of the largest samples of global web traffic available,
and are refreshed hourly.

### 4.9 Infrastructure resilience

The Internet Society's Internet Resilience Index is presented for approximately 179
countries, across its four pillars: infrastructure, performance, security, and market
readiness. It is updated quarterly.

The index measures how robust a country's internet is, meaning how well it would hold up
under stress. It bears on shutdowns only indirectly. A country whose connectivity depends
on a few providers and a few routes is both more fragile and easier to cut off.

### 4.10 Physical and orbital infrastructure

- **Undersea cables**: routes and landing points, from TeleGeography, shown as a layer on the
  map. A country reached by a single cable landing faces a fundamentally different situation
  from one reached by twelve. Fewer landing points means fewer chokepoints to control.
- **Internet exchange points**: the facilities where networks connect to swap traffic
  locally. The tool shows how many a country has and how many networks connect to them,
  from PeeringDB. A country with few exchange points routes most of its traffic through a
  handful of international gateways. That makes a shutdown quicker and cheaper to impose,
  and makes it more likely that cutting international links also cuts the country off from
  itself.
- **Starlink restrictions**: the countries where Starlink satellite internet is known to be
  banned, restricted, or jammed (25 countries as of September 2026). Satellite service is
  the most consequential recent change in whether national shutdowns can be enforced at all.
  The list is compiled by hand from public sources, which it names, because no single
  attributable feed exists. It records known restrictions; countries without a recorded
  restriction are not flagged.
- **Satellite positions**: positions of satellites overhead, calculated in real time from
  published orbital data (CelesTrak and SatNOGS) that is refreshed every two hours.

---

## 5. Principles governing the data

Five commitments govern how BLACKOUT handles every figure it presents.

**One direction for every scale.** Every index score in the tool is presented so that a
higher number means more freedom, regardless of how its source publishes it. Sources using
the opposite convention are converted. Inconsistent scale direction is among the most common
causes of misreading in multi-source analysis, and it is eliminated here by design.

**Missing data is reported as missing.** A country or platform without sufficient
measurements is never shown as unrestricted. Absence of evidence is never rendered as
evidence of absence. Throughout the
tool, a visible gap is treated as preferable to a confident figure that may be wrong.

**Uncertainty is preserved.** Where a source publishes a range rather than a single figure,
that range is carried through and made available. Where a conclusion depends on how much
evidence exists, the amount of evidence forms part of the conclusion rather than a footnote
to it.

**Every figure remains traceable to its origin.** Each display names its source. The
composite index shows its components. No number in the tool is presented without the means
to trace it back.

**Nothing is submitted by users.** BLACKOUT is read-only and has no accounts, no submission
mechanism, and no way for a visitor to influence what is displayed. Everything shown derives
from named external sources.

Beyond these commitments, each source is refreshed on a schedule suited to how quickly it
changes: network measurement and outage data every six hours, satellite data every two
hours, and routing visibility and web protocol share every hour. Annual and quarterly
indices, and reference datasets, are updated as new editions become available. The tool
displays the age of its data, and flags it as stale if it falls out of date. Data currency is
mixed by design, and §7 states the currency of each source rather than implying a uniform
one.

---

## 6. Using the findings responsibly

**Corroborate before concluding.** The strongest findings BLACKOUT supports are those where
independent indicators agree. A detected outage, a collapse in routing visibility, and a
spike in measured blocking in the same country over the same period together constitute
well-evidenced disruption. Any one of them alone is a line of inquiry, not a conclusion.

**Treat divergence as a finding in itself.** A country with a moderate index score but heavy
measured restriction of AI services is telling you something real, often that a specific,
recent policy has outpaced the general assessment of its information environment. This is
visible only because the index and the observed indicators are kept separate.

**Check coverage before comparing countries.** Network measurement depends on volunteers
running measurement software, and that coverage is uneven. Two countries are not equally
well observed simply because both appear on the map. Where the tool reports insufficient
data, that means the question is open, not that the answer is favourable.

**Check the basis of each score.** The components behind an index score, and the year of
each assessment, are both displayed and both affect comparability.

**Cite the original source for source data.** Figures originating with V-Dem, RSF, or any
other contributing organisation should be cited to that organisation. BLACKOUT should be
cited for the composite index and for the integrated analysis it makes possible.

---

## 7. Sources

| Source | Contribution | How current |
| --- | --- | --- |
| OONI (Open Observatory of Network Interference) | Platform blocking, messaging availability, content-category censorship, blocking history | Refreshed every 6 hours |
| IODA (Georgia Tech) | National internet outage events | Refreshed every 6 hours |
| Tor Project | Tor and bridge usage by country | Daily figures, refreshed every 6 hours |
| Cloudflare | Web protocol share | Daily figures, refreshed hourly |
| RIPE NCC | Routing visibility | Refreshed hourly |
| Internet Society | Internet Resilience Index | Quarterly |
| V-Dem Institute *(via Our World in Data)* | Freedom of Expression Index (**index component**) | Annual; latest year 2025 |
| V-Dem Institute | Internet filtering, shutdown, and censorship indicators | 2025 release |
| Reporters Without Borders *(via Our World in Data)* | Press Freedom Index, pre-2022 edition (**index component**) | Annual; final year 2021 |
| PeeringDB | Internet exchange points | Reference snapshot, September 2026 |
| TeleGeography | Undersea cable routes and landing points | Reference snapshot, September 2026 |
| CelesTrak / SatNOGS | Satellite orbital data | Refreshed every 2 hours |
| Compiled from public sources | Starlink restrictions by country | Curated by hand, sources named; reviewed September 2026 |

---

## 8. Summary

BLACKOUT's value does not lie in measuring something new. It lies in making four
incompatible kinds of evidence — expert political assessment, direct network measurement,
routing evidence, and physical infrastructure — readable together, for nearly every country
in the world, without concealing what any figure is built from.

The Censorship Index is BLACKOUT's own construction: a transparent weighted combination of
two established international indices, oriented so that every index score in the tool runs
in the same direction, and always published alongside the components that produced it. It
describes a country's general environment, and it is deliberately held apart from the observed network
indicators so that disagreements between expert judgement and direct measurement remain
visible rather than being averaged away.

Every figure in the tool is observable, sourced, and traceable to its origin. That is the
standard BLACKOUT holds itself to, and it is why its output can responsibly be placed in
front of those who must act on it.
