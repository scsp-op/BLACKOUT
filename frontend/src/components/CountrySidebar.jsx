import { useEffect, useState } from 'react'
import BgpVisibilityChart from './BgpVisibilityChart'
import CategoryBreakdown from './CategoryBreakdown'
import GlobalIndices from './GlobalIndices'
import Http3ShareChart from './Http3ShareChart'
import MessagingStatus from './MessagingStatus'
import OutageTimeline from './OutageTimeline'
import ResilienceIndex from './ResilienceIndex'
import StarlinkBadge from './StarlinkBadge'
import IxpBadge from './IxpBadge'
import TimelineChart from './TimelineChart'
import TorChart from './TorChart'
import {
  BLOCKING_REGISTRY,
  BLOCKING_STATUS_COLOR,
  BLOCKING_STATUS_LABEL,
  GROUP_LABELS,
  hasTimeline,
} from '../lib/blockingRegistry'
import { BORDER, BORDER_STRONG, MONO, MUTED, SIDEBAR, WHITE } from '../theme'

const ALL_TECHNOLOGIES = Object.values(BLOCKING_REGISTRY).flat()

// A visual "theme" is one level above the small per-widget MONO headers
// (BLOCKING STATUS, MESSAGING APPS, etc.) — it groups related signals (access
// blocking, censorship, network/protocol, resilience) behind a bolder label
// and a real divider line, so unrelated signals don't read as one continuous
// blob when they only had an 18px gap between them.
//
// paddingBottom matters as much as paddingTop here: with only paddingTop, the
// divider line sits flush against whatever the *previous* section's last
// widget happens to render (a quiet footnote vs. a dense row of pill badges),
// so the same 16px gap reads as "loose" after one widget and "cramped" after
// another. Padding both sides guarantees a fixed quiet zone around the line
// itself, independent of what content sits on either side of it.
function ThemeSection({ title, first, children }) {
  return (
    <section
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 18,
        paddingTop: first ? 0 : 16,
        paddingBottom: 16,
        borderTop: first ? 'none' : `1px solid ${BORDER_STRONG}`,
      }}
    >
      <div style={{ fontFamily: MONO, fontSize: 11, letterSpacing: '0.14em', color: WHITE }}>{title}</div>
      {children}
    </section>
  )
}

function BlockSegments({ filledCount, color }) {
  return (
    <div style={{ display: 'flex', gap: 1, flexShrink: 0 }}>
      {Array.from({ length: 8 }).map((_, i) => (
        <div key={i} style={{ width: 6, height: 6, background: i < filledCount ? color : BORDER }} />
      ))}
    </div>
  )
}

// Days of history a timeline needs before it counts as a signal on its own.
//
// `> 0` was too low to mean anything. OONI publishes the occasional isolated
// day for a technology it barely covers in a country, and one such row was
// enough to promote the technology, resurrect its whole group, and add a
// group heading — Iran had exactly one such day against 400-990 days
// for every technology actually tracked there. The resulting chart is a
// single point, which draws no line at all, under a "1 days · peak ..."
// footnote.
//
// A week is the smallest window where a sparkline shows a shape rather than a
// dot, and it sits far below the real technologies' hundreds of days, so it
// separates noise from signal without hiding anything genuine.
const MIN_TIMELINE_DAYS = 7

// A row is worth showing only if it carries an actual signal: either the
// point-in-time classification resolved to something other than
// inconclusive/no-data, or there's enough historical timeline behind it. A
// blank "INCONCLUSIVE / 0" row tells a reader nothing and just adds noise.
function isMeaningful(row, timelineRows) {
  const hasPointSignal = !!row && row.measurement_count > 0 && row.status !== 'INCONCLUSIVE'
  return hasPointSignal || hasEnoughTimeline(timelineRows)
}

function hasEnoughTimeline(timelineRows) {
  return (timelineRows?.length ?? 0) >= MIN_TIMELINE_DAYS
}

// "Still fetching" and "there is genuinely nothing here" used to render
// identically — as nothing at all — so a country with complete data looked
// broken for the whole of a cold start. That window is minutes, not
// milliseconds: OONI's timeline phase is the slowest thing in the app (ten
// 2-D aggregations over every country since 2024-01-01, up to ~18 MB each),
// and until it lands every blocking section is empty.
//
// This is the same distinction the pulse fetcher already insists on in its
// own boot warning — an empty panel has to say whether it is empty because
// the data has not arrived or because the source does not cover this country.
function SectionState({ loading, emptyLabel }) {
  return (
    <p style={{ fontFamily: MONO, fontSize: 9, letterSpacing: '0.1em', color: MUTED }}>
      {loading ? 'LOADING\u2026' : emptyLabel}
    </p>
  )
}

function BlockingGroupList({ groups, blockingByTech, timelineByTech, countryCode, showGroupLabel = true }) {
  return (
    <>
      {groups.map(({ group, techs }, i) => (
        <div
          key={group}
          style={{
            marginBottom: 10,
            paddingTop: i === 0 ? 0 : 8,
            borderTop: i === 0 ? 'none' : `1px solid ${BORDER}`,
          }}
        >
          {showGroupLabel && (
            <p
              style={{
                fontFamily: MONO,
                fontSize: 9,
                letterSpacing: '0.1em',
                textTransform: 'uppercase',
                color: MUTED,
                marginBottom: 2,
              }}
            >
              {GROUP_LABELS[group]}
            </p>
          )}
          {techs.map((tech) => (
            <BlockingTechRow
              key={tech}
              tech={tech}
              row={blockingByTech[tech]}
              countryCode={countryCode}
              timelineRows={timelineByTech[tech]}
            />
          ))}
        </div>
      ))}
    </>
  )
}

function BlockingTechRow({ tech, row, countryCode, timelineRows }) {
  const status = row?.status ?? 'NO_DATA'
  const count = row?.measurement_count ?? 0
  const anomalyRate = row?.anomaly_rate ?? 0
  const color = BLOCKING_STATUS_COLOR[status] ?? BORDER
  const filledCount = Math.max(0, Math.min(8, Math.round(anomalyRate * 8)))
  const showTimeline = hasTimeline(countryCode, tech) && hasEnoughTimeline(timelineRows)

  return (
    <div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, height: 22 }}>
        <span style={{ fontFamily: MONO, fontSize: 10, color: WHITE, width: 100, flexShrink: 0 }}>{tech}</span>
        <BlockSegments filledCount={filledCount} color={color} />
        <span style={{ fontFamily: MONO, fontSize: 10, color, width: 72, flexShrink: 0 }}>
          {BLOCKING_STATUS_LABEL[status]}
        </span>
        <span style={{ fontFamily: MONO, fontSize: 10, color: MUTED }}>{count}</span>
      </div>
      {showTimeline && (
        <div style={{ padding: '6px 0 6px 0' }}>
          <TimelineChart rows={timelineRows} />
        </div>
      )}
    </div>
  )
}

export default function CountrySidebar({ country, layer, starlinkStatus, ixpStats, onClose }) {
  const [blockingRows, setBlockingRows] = useState([])
  const [blockingLoading, setBlockingLoading] = useState(true)
  const [timelineByTech, setTimelineByTech] = useState({})
  const [timelineLoading, setTimelineLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    setBlockingRows([])
    setBlockingLoading(true)

    fetch(`/api/blocking?country=${country.country_code}`)
      .then((response) => (response.ok ? response.json() : []))
      .then((rows) => {
        if (!cancelled) setBlockingRows(rows)
      })
      .catch(() => {
        if (!cancelled) setBlockingRows([])
      })
      .finally(() => {
        if (!cancelled) setBlockingLoading(false)
      })

    return () => {
      cancelled = true
    }
  }, [country.country_code])

  useEffect(() => {
    let cancelled = false
    setTimelineByTech({})
    setTimelineLoading(true)

    const promoted = ALL_TECHNOLOGIES.filter((tech) => hasTimeline(country.country_code, tech))
    Promise.all(
      promoted.map((tech) =>
        fetch(`/api/timeline?country=${country.country_code}&technology=${tech}`)
          .then((response) => (response.ok ? response.json() : []))
          .then((rows) => [tech, rows])
          .catch(() => [tech, []]),
      ),
    ).then((entries) => {
      if (!cancelled) {
        setTimelineByTech(Object.fromEntries(entries))
        setTimelineLoading(false)
      }
    })

    return () => {
      cancelled = true
    }
  }, [country.country_code])

  const blockingByTech = Object.fromEntries(blockingRows.map((row) => [row.technology, row]))
  const groupsForLayer = layer === 'AI_ACCESS' || layer === 'CIRCUMVENTION'
    ? [layer]
    : ['AI_ACCESS', 'CIRCUMVENTION']

  const visibleGroups = groupsForLayer
    .map((group) => ({
      group,
      techs: BLOCKING_REGISTRY[group].filter((tech) => isMeaningful(blockingByTech[tech], timelineByTech[tech])),
    }))
    .filter(({ techs }) => techs.length > 0)

  const aiAccessGroups = visibleGroups.filter(({ group }) => group === 'AI_ACCESS')
  const circumventionGroups = visibleGroups.filter(({ group }) => group === 'CIRCUMVENTION')

  // A row is only considered settled once BOTH fetches are in: `isMeaningful`
  // promotes a technology on either a point-in-time classification or a
  // timeline, so a group that looks empty with only one of the two loaded may
  // still fill in.
  const blockingPending = blockingLoading || timelineLoading
  // Which sections the current layer filter admits at all. Without this, a
  // CIRCUMVENTION-only view would report "no coverage" for AI access, when in
  // fact it was simply not asked for.
  const showAiAccess = groupsForLayer.includes('AI_ACCESS')
  const showCircumvention = groupsForLayer.includes('CIRCUMVENTION')

  return (
    <div
      style={{
        width: 380,
        minWidth: 340,
        height: '100%',
        overflowY: 'auto',
        background: SIDEBAR,
        borderLeft: `1px solid ${BORDER}`,
      }}
    >
      <div style={{ padding: '16px 20px', borderBottom: `1px solid ${BORDER}` }}>
        <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between' }}>
          <div style={{ display: 'flex', alignItems: 'baseline', gap: 8 }}>
            <h2 style={{ fontSize: 14, fontWeight: 500, color: WHITE }}>{country.country_name}</h2>
            <span style={{ fontFamily: MONO, fontSize: 11, letterSpacing: '0.05em', color: MUTED }}>
              {country.country_code}
            </span>
          </div>
          <button
            onClick={onClose}
            style={{ background: 'transparent', border: 'none', color: MUTED, fontSize: 18, lineHeight: 1, cursor: 'pointer' }}
          >
            ×
          </button>
        </div>
      </div>

      <div style={{ padding: '16px 20px', display: 'flex', flexDirection: 'column', gap: 0 }}>
        <ThemeSection title="NETWORK & PROTOCOL" first>
          <OutageTimeline countryCode={country.country_code} />

          <Http3ShareChart countryCode={country.country_code} />

          <BgpVisibilityChart countryCode={country.country_code} />

          <IxpBadge entry={ixpStats} />

          <StarlinkBadge entry={starlinkStatus} />
        </ThemeSection>

        {showAiAccess && (
          <ThemeSection title="AI ACCESS">
            <div>
              <div style={{ fontFamily: MONO, fontSize: 10, letterSpacing: '0.1em', color: MUTED, marginBottom: 8 }}>
                BLOCKING STATUS
              </div>
              {aiAccessGroups.length > 0 ? (
                <BlockingGroupList
                  groups={aiAccessGroups}
                  blockingByTech={blockingByTech}
                  timelineByTech={timelineByTech}
                  countryCode={country.country_code}
                  showGroupLabel={false}
                />
              ) : (
                <SectionState loading={blockingPending} emptyLabel="NO OONI COVERAGE" />
              )}
            </div>
          </ThemeSection>
        )}

        <ThemeSection title="CENSORSHIP">
          <CategoryBreakdown countryCode={country.country_code} />
        </ThemeSection>

        <ThemeSection title="MESSAGING">
          <MessagingStatus countryCode={country.country_code} />
        </ThemeSection>

        <ThemeSection title="CIRCUMVENTION">
          {showCircumvention && (
            <div>
              <div style={{ fontFamily: MONO, fontSize: 10, letterSpacing: '0.1em', color: MUTED, marginBottom: 8 }}>
                BLOCKING STATUS
              </div>
              {circumventionGroups.length > 0 ? (
                <BlockingGroupList
                  groups={circumventionGroups}
                  blockingByTech={blockingByTech}
                  timelineByTech={timelineByTech}
                  countryCode={country.country_code}
                  // One group here, so a label would only restate the
                  // section title.
                  showGroupLabel={false}
                />
              ) : (
                <SectionState loading={blockingPending} emptyLabel="NO OONI COVERAGE" />
              )}
            </div>
          )}

          {/* Tor relay/bridge usage. Rendered from its own /api/tor-metrics data
              (it returns null when empty) rather than gated on a "meaningful" tor
              blocking row, so it surfaces for every country that has Tor data —
              not just those with a point-in-time blocking classification. */}
          <TorChart countryCode={country.country_code} />
        </ThemeSection>

        <ThemeSection title="RESILIENCE & FREEDOM INDICES">
          <ResilienceIndex countryCode={country.country_code} />

          <GlobalIndices countryCode={country.country_code} />
        </ThemeSection>
      </div>
    </div>
  )
}
