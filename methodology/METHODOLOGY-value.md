# BLACKOUT — Methodology and Analytical Value

*Prepared September 2026. This document describes what BLACKOUT measures, how its
Censorship Index is constructed, and how its findings should and should not be used. A
separate technical methodology document covers data collection, system architecture, and
implementation.*

---

## 1. Overview

BLACKOUT is an open-source analytical tool that presents internet censorship, network
disruption, and information freedom for every country in the world in a single view.

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
alongside the circumvention tools that have long been monitored. This is the tool's most
distinctive contribution.

**Network shutdowns have become routine instruments of governance** rather than exceptional
events. Annual expert assessments, however rigorous, cannot capture a disruption that
begins on a Tuesday and ends on a Thursday. Continuous observation is now a requirement, not
a refinement.

**Circumvention has moved to space and to the seabed.** Satellite internet availability,
submarine cable landings, and the density of domestic exchange points increasingly determine
whether a government's restriction can actually be enforced. These physical constraints
belong in the same picture as the political assessments, because in practice they limit each
other.

### Questions the tool is built to answer

| Question | What BLACKOUT provides |
| --- | --- |
| How restricted is this country overall? | A composite Censorship Index, with its component scores shown alongside it |
| Is access to AI services restricted here? | Direct measurement of reachability for major AI platforms |
| Is this a deliberate restriction or a technical failure? | Three independent signals — outage detection, routing evidence, and network measurement — which can be compared |
| Is the situation deteriorating? | Ninety days of outage history and daily blocking records since January 2024 |
| Could a shutdown here even be enforced? | Cable landings, exchange point density, satellite availability, and circumvention-tool demand |
| How does this country compare globally? | A worldwide ranking across all assessed countries |

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
citable. The combination is BLACKOUT's own, and its value is in making two indices that use
incompatible scales directly comparable and mappable for the first time.

### 3.2 What goes into it

| Component | Produced by | What it assesses | Share of score |
| --- | --- | --- | --- |
| Freedom of Expression Index | V-Dem Institute (University of Gothenburg) | Freedom of expression and the availability of alternative sources of information, assessed by country experts | 62.5% |
| Press Freedom Index | Reporters Without Borders (RSF) | Media pluralism and independence, the legislative environment, and the safety of journalists | 37.5% |

Both are drawn from Our World in Data, which republishes these indices in a standardised,
country-by-country form. The tool links to those specific published datasets rather than to
the originating organisations generally, because that is where the figures it displays
actually come from.

Each index covers roughly 180 countries, which is what allows BLACKOUT to present a
worldwide baseline rather than a selected group of countries. For each country, the most
recent available year is used, and that year is displayed alongside the score.

**Freedom of expression is weighted more heavily than press freedom** for three reasons: it
addresses the broader question, it has somewhat better country coverage, and the press
freedom series available in standardised form is an older edition (see §3.6).

### 3.3 Putting the two on one scale

The two indices do not share a scale, and — critically — they do not run in the same
direction.

The V-Dem index already treats higher scores as more free. The RSF series, in the edition
used here, does the opposite: it measures abuse, so a higher score means a *worse*
environment. BLACKOUT reverses the RSF figure before combining it.

This correction is essential rather than cosmetic. Without it, every press freedom
contribution would pull in precisely the wrong direction, and the resulting composite would
rank the freest countries as the most repressive.

Both components are then placed on a common 0–100 scale on which **a higher score means more
freedom**. This convention holds throughout the entire tool, for every score it displays,
from every source. The single exception is the world map itself, which necessarily shades by
restriction rather than freedom, and which labels its scale accordingly.

Each component also carries the descriptive classification published by its own
organisation — V-Dem's bands from "Very free" to "Highly repressed," and RSF's from "Good"
to "Very serious." These are the source organisations' own judgments, applied on their own
original scales, and BLACKOUT does not alter them.

### 3.4 How the two are combined

The composite is a weighted average of the two components, in a ratio of roughly five to
three in favour of freedom of expression. The resulting freedom score is then subtracted
from 100 to express it as a censorship score, which is what the map shades by.

Three features of this method matter for interpretation:

**A country missing one component still receives a score.** Where only one of the two
indices covers a country, the composite is calculated from that component alone rather than
leaving the country unscored. This is what gives the index near-global coverage instead of
limiting it to countries both organisations assess.

The trade-off is that a score resting on one source is a weaker estimate than one resting on
two. BLACKOUT therefore publishes, for every country, **how many components its score is
based on**, so that users can restrict their analysis to fully covered countries where the
question demands it.

**A country covered by neither index receives no score at all** and is left unshaded on the
map. The tool does not fill gaps with estimates.

**The composite never obscures its inputs.** Both component scores are displayed alongside
the composite for every country. A user who disagrees with BLACKOUT's weighting has
everything needed to apply their own. The index is offered as a transparent starting point
for analysis, not as an authority to be accepted on trust.

### 3.5 Internet-specific censorship indicators

Beneath the headline freedom of expression score, BLACKOUT displays three further V-Dem
indicators that address internet censorship directly:

- **Government internet filtering** — the extent to which a government filters online
  content in practice
- **Freedom from internet shutdowns** — the extent to which a government refrains from
  shutting down internet access
- **Government censorship effort** — the extent of a government's effort to censor online
  information

These are frequently the most decision-relevant figures in the tool, because they speak to
state conduct rather than to the general information environment.

**They are deliberately not folded into the composite index.** They are presented separately
so that a user can see where a country's general political environment and its specific
internet conduct diverge — which is often the most informative thing on the screen.

Each of these indicators is published by V-Dem with a stated range of uncertainty rather
than as a single definitive figure. BLACKOUT preserves that range and makes it available
alongside the estimate, rather than presenting a point estimate as more precise than its
source claims it to be.

**These three indicators are fixed at 2025** and cover 179 countries. Unlike the headline
scores, which advance to each country's most recent available year, these do not change
until the underlying dataset is deliberately updated. Users should not read them as evidence
of recent change, and should be aware that an internet-specific indicator may not be from
the same year as the headline score displayed above it.

### 3.6 Limitations of the index

These should be understood before the index is cited.

**It is an index built from other indices.** It inherits the limitations of both sources,
including their reliance on expert judgment and their annual rather than continuous
publication. It describes a country's general environment. It does not describe what that
country's network did this week — for that, the observed indicators in §4 are the
appropriate evidence.

**The press freedom component is a superseded edition.** RSF revised its methodology in
2022, and the standardised series used here predates that revision. Comparisons spanning
that methodological break are not supported by this data.

**Single-source scores are weaker.** The number of components behind each score is
published for exactly this reason and should be checked before two countries are treated as
equally well-founded.

**The weighting is a reasoned editorial judgment, not an empirical derivation.** The
five-to-three ratio has not been statistically fitted or validated against an external
outcome. It is stated openly so that it can be questioned, and both components are published
so that an alternative weighting can be applied.

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

BLACKOUT tracks whether major AI platforms — OpenAI, Claude, DeepSeek, and Hugging Face —
are reachable from within each country, alongside the established circumvention tools: Tor,
Signal, Psiphon, I2P, and Snowflake.

Reachability is assessed from network measurements taken inside each country over the
preceding 90 days, so that a restriction lifted long ago is not reported as current.

A platform is reported as **blocked** only when the substantial majority of tests fail
across enough tests to be meaningful. Where failures are present but the evidence is thinner
or more mixed, the result is reported as **likely blocked** or as **inconclusive**. Where too
few measurements exist to support any conclusion, the tool reports that directly rather than
inferring availability from silence.

This conservatism is deliberate: a small number of failed tests can reflect ordinary network
trouble rather than state action, and reporting that as censorship would be a serious error.

### 4.2 Messaging platform availability

Availability of WhatsApp, Telegram, Facebook Messenger, and Signal is tracked separately,
and assessed more strictly.

These platforms are measured by tests designed specifically for each service, which examine
whether the service itself is reachable — not merely whether its public website loads. The
distinction matters in practice: a government can leave a messaging company's website fully
accessible while blocking the service its application depends on, and a simpler test would
record that as unrestricted.

Messaging availability is reported as blocked only on confirmed evidence of blocking.
Where irregularities appear without confirmation, the result is reported as inconclusive.
Messaging disruption is exactly the kind of finding where a false positive would be most
damaging, and the threshold is set accordingly. A confidence level based on how much
evidence exists accompanies every assessment.

### 4.3 Categories of censored content

The same body of measurement is also assessed by subject matter, showing which categories of
content — news media, political criticism, human rights material, and others — are
restricted in each country over the preceding six months.

Category-level assessment uses different thresholds from single-platform assessment, because
a category covers hundreds of individual sites, most of which usually remain reachable even
where a subject is heavily targeted. Applying single-platform thresholds here would make
almost every country appear unrestricted.

### 4.4 Blocking history

Daily records since January 2024 show how restrictions on each tracked platform have changed
over time, for every country where measurements exist. This converts a point-in-time
assessment into a trend, which is usually the more useful form for policy work.

### 4.5 Internet outages

Country-level outage events over the preceding 90 days are drawn from IODA, a research
project at Georgia Tech and CAIDA that detects national disruptions by combining four
independent signals: global routing data, active network probing, observation of unsolicited
internet traffic, and public traffic reporting.

Each event records when it began, how long it lasted, how severe it was, and which signal
detected it. That last detail carries analytical weight, because the signals fail in
different ways — an outage visible in routing data but not in traffic measurement means
something different from the reverse.

### 4.6 Routing visibility

For each country, BLACKOUT compares how much of its allocated internet address space is
**actually announced to the global internet** against how much is merely **registered to
it**, using data from the RIPE Network Coordination Centre.

This is the indicator that most clearly distinguishes a deliberate disconnection from a
technical failure. When a government instructs networks to withdraw their routing
announcements, the announced figure collapses while the registered figure remains
unchanged — a signature that ordinary infrastructure failure does not produce.

### 4.7 Circumvention demand

Usage of the Tor network — both direct connections and the concealed entry points used where
Tor itself is blocked — is tracked by country.

This is best read as a **demand signal**. A sharp rise in the use of concealed access
methods in a given country is evidence that ordinary access is being interfered with,
independent of whether any measurement probe happened to be operating there at the time. It
is a useful corrective in countries where direct measurement coverage is thin.

### 4.8 Traffic composition

The mix of internet protocols in use in each country is tracked using data from Cloudflare,
on a shorter refresh cycle than most other indicators.

This serves as an early indicator. Shifts in the protocol mix can accompany or precede the
deployment of filtering infrastructure, sometimes before any blocking is directly observed.

### 4.9 Infrastructure resilience

The Internet Society's Internet Resilience Index is presented for approximately 179
countries, across its four dimensions: infrastructure, performance, security, and market
readiness.

This addresses the underlying question of capability: if a government wished to restrict or
sever access, how difficult would it be, and how much would it cost?

### 4.10 Physical and orbital infrastructure

- **Submarine cables** — routes and landing points. A country served by a single landing
  station faces a fundamentally different situation from one served by twelve. Chokepoints
  are structural capability for control.
- **Internet exchange points** — how much of a country's traffic can remain domestic. This
  determines whether severing international connectivity also severs a country from itself.
- **Satellite internet availability** — where satellite service is permitted, restricted, or
  unavailable. This is the single most consequential recent change in whether national
  shutdowns can be enforced at all. This dataset is compiled by hand from multiple public
  sources and cites those sources directly, as it does not derive from a single attributable
  feed.
- **Satellite positions** — live positions of satellites overhead, shown on the same globe.

---

## 5. Principles governing the data

Five commitments govern how BLACKOUT handles every figure it presents.

**One direction for every scale.** Every score in the tool is presented so that a higher
number means more freedom, regardless of how its source publishes it. Sources using the
opposite convention are converted. Inconsistent scale direction is among the most common
causes of misreading in multi-source analysis, and it is eliminated here by design.

**Missing data is reported as missing.** A country with no measurements is shown as having
no data, never as unrestricted. Absence of evidence is never rendered as evidence of
absence. Throughout the tool, a visible gap is treated as preferable to a confident figure
that may be wrong.

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

Beyond these commitments, the tool refreshes continuously throughout each day, reports
itself as stale rather than silently serving outdated figures, and displays the age of the
data alongside it. Data currency is mixed by design — some sources update continuously,
some are periodic reference datasets, some are maintained by hand — and the tool states
which is which rather than implying a uniform currency it does not have.

---

## 6. Using the findings responsibly

**Corroborate before concluding.** The strongest findings BLACKOUT supports are those where
independent indicators agree. A detected outage, a collapse in routing visibility, and a
spike in measured blocking in the same country over the same period together constitute
well-evidenced disruption. Any one of them alone is a line of inquiry, not a conclusion.

**Treat divergence as a finding in itself.** A country with a moderate index score but heavy
measured restriction of AI services is telling you something real: that a specific, recent
policy has outpaced the general assessment of its information environment. This is visible
only because the index and the observed indicators are kept separate.

**Check coverage before comparing countries.** Network measurement depends on volunteers
running measurement software, and that coverage is uneven. Two countries are not equally
well observed simply because both appear on the map. Where the tool reports insufficient
data, that means the question is open, not that the answer is favourable.

**Check the basis of each score.** The number of components behind an index score, and the
year of each assessment, are both displayed and both affect comparability.

**Cite the original source for source data.** Figures originating with V-Dem, RSF, or any
other contributing organisation should be cited to that organisation. BLACKOUT should be
cited for the composite index and for the integrated analysis it makes possible.

---

## 7. Sources

| Source | Contribution | Currency |
| --- | --- | --- |
| OONI (Open Observatory of Network Interference) | Platform blocking, messaging availability, content-category censorship, blocking history | Continuously updated |
| IODA (Georgia Tech / CAIDA) | National internet outage events | Continuously updated |
| Tor Project | Circumvention tool usage by country | Continuously updated |
| Cloudflare | Traffic composition, outage corroboration | Continuously updated |
| RIPE NCC | Routing visibility | Continuously updated |
| Internet Society | Internet Resilience Index | Continuously updated |
| V-Dem Institute *(via Our World in Data)* | Freedom of Expression Index — **index component** | Continuously updated |
| V-Dem Institute | Internet filtering, shutdown, and censorship indicators | Fixed at 2025 |
| Reporters Without Borders *(via Our World in Data)* | Press Freedom Index — **index component** | Continuously updated |
| PeeringDB | Internet exchange point density | Periodic reference dataset |
| TeleGeography | Submarine cable routes and landing points | Periodic reference dataset |
| CelesTrak / SatNOGS | Satellite positions | Continuously updated |
| Compiled from public sources | Satellite internet availability by country | Maintained by hand, sources cited |

---

## 8. Summary

BLACKOUT's value does not lie in measuring something new. It lies in making four
incompatible kinds of evidence — expert political assessment, direct network measurement,
routing-level ground truth, and physical infrastructure — readable together, on one scale,
for every country in the world, without concealing what any figure is built from.

The Censorship Index is BLACKOUT's own construction: a transparent weighted combination of
two established international indices, oriented so that every measure in the tool runs in the
same direction, and always displayed alongside the components that produced it. It describes
a country's general environment, and it is deliberately held apart from the observed network
indicators so that disagreements between expert judgment and direct measurement remain
visible rather than being averaged away.

Every figure in the tool is observable, sourced, and traceable to its origin. That is the
standard BLACKOUT holds itself to, and it is why its output can responsibly be placed in
front of those who must act on it.
