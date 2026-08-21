import type { Graph } from '$lib/api';

/**
 * The owner's live corpus, as the graph route actually receives it.
 *
 * Thirty-five pages with their real titles, extracted from `content-darm/` on 2026-08-21,
 * because the label defect this fixture exists to measure is a function of how LONG these
 * titles are and no invented fixture is honest about that: they run from 20 to 84
 * characters, they are German and English mixed, and half of them are wider than a quarter
 * of the drawing when set at 13px. `N0`-style placeholders make the graph look fine.
 *
 * The edges are this corpus's real links, plus one link to its section for each page that
 * had none — sixteen of them — so that all thirty-five pages appear. That is not padding:
 * `Store::graph_for` drops a node no surviving edge touches, so the graph of the corpus as
 * written today is nineteen nodes, and the thirty-five-node picture is what the route draws
 * once the pages are cross-linked, which is the size the owner is migrating for. Linking a
 * page up to its section is also the commonest link in this corpus — ten pages already do
 * exactly that.
 *
 * Ordered the way the API orders it, so the layout it feeds is the layout a reader gets:
 * nodes by path, edges by the path of each end. See `Store::graph_for` in
 * `crates/gw-store/src/links.rs`.
 */
export const CORPUS: Graph = {
  nodes: [
    { path: '/darm', title: 'Darm — ADHD Microbiota Reference' },
    { path: '/darm/adhs-darmbakterien-phaenotyp', title: 'ADHS Darmbakterien Phänotyp' },
    { path: '/darm/detaillierte-bakterienprofile', title: 'Detaillierte Bakterienprofile — Tabelle 1' },
    { path: '/darm/detaillierte-bakterienprofile/erhoehte-bakterienarten', title: 'Erhöhte Bakterienarten (↑)' },
    { path: '/darm/detaillierte-bakterienprofile/reduzierte-bakterienarten', title: 'Reduzierte Bakterienarten (↓)' },
    { path: '/darm/detaillierte-interventionsstrategien', title: 'Detaillierte Interventionsstrategien — Tabelle 5' },
    { path: '/darm/detaillierte-interventionsstrategien/4-1-prenatal-optimization-maternal-foundation', title: '4.1 Prenatal Optimization (Maternal Foundation)' },
    { path: '/darm/detaillierte-interventionsstrategien/4-2-infancy-birth-to-6-months-critical-neonatal-colonization', title: '4.2 Infancy (Birth to 6 Months): Critical Neonatal Colonization' },
    { path: '/darm/detaillierte-interventionsstrategien/4-3-toddlerhood-6-months-to-2-years-microbiota-diversification', title: '4.3 Toddlerhood (6 Months to 2 Years): Microbiota Diversification' },
    { path: '/darm/detaillierte-interventionsstrategien/4-4-early-childhood-2-to-6-years-dietary-pattern-establishment', title: '4.4 Early Childhood (2 to 6 Years): Dietary Pattern Establishment' },
    { path: '/darm/detaillierte-interventionsstrategien/4-5-school-age-child-6-years-two-pathways', title: '4.5 School-Age Child (6+ Years) — Two Pathways' },
    { path: '/darm/detaillierte-interventionsstrategien/4-6-medication-effects-on-microbiota-counteractive-strategies', title: '4.6 Medication Effects on Microbiota — Counteractive Strategies' },
    { path: '/darm/detaillierte-interventionsstrategien/4-7-antibiotic-exposure-recovery-protocol', title: '4.7 Antibiotic Exposure Recovery Protocol' },
    { path: '/darm/detaillierte-interventionsstrategien/critical-window-insight', title: 'Critical Window Insight' },
    { path: '/darm/key-prevention-principles', title: 'Key Prevention Principles' },
    { path: '/darm/probiotika-im-vergleich-tabelle', title: 'Probiotika im Vergleich Tabelle' },
    { path: '/darm/quellen-referenzen', title: 'Quellen & Referenzen' },
    { path: '/darm/table-0-dysbiotic-shifts-within-phyla', title: 'Table 0: Dysbiotic Shifts Within Phyla and Families' },
    { path: '/darm/table-1-bacterial-dysbiosis-summary', title: 'Table 1: Bacterial Dysbiosis Summary — Targets, Interventions, Effects' },
    { path: '/darm/table-2-clinical-monitoring-biomarkers', title: 'Table 2: Clinical Monitoring Biomarkers & Restoration Targets' },
    { path: '/darm/table-2-clinical-monitoring-biomarkers/2-1-microbiota-composition-16s-rrna-sequencing', title: '2.1 Microbiota Composition (16S rRNA Sequencing)' },
    { path: '/darm/table-2-clinical-monitoring-biomarkers/2-2-scfa-targets-fecal-gc-ms-plasma', title: '2.2 SCFA Targets — Fecal (GC-MS) & Plasma' },
    { path: '/darm/table-2-clinical-monitoring-biomarkers/2-3-symptom-functional-endpoints', title: '2.3 Symptom / Functional Endpoints' },
    { path: '/darm/table-3-scfa-dysbalance', title: 'Table 3: SCFA Dysbalance' },
    { path: '/darm/table-3-scfa-dysbalance/3-1-scfa-levels-producers-neurotransmitter-impact', title: '3.1 SCFA Levels, Producers & Neurotransmitter Impact' },
    { path: '/darm/table-3-scfa-dysbalance/3-2-key-mechanisms-adhd-relevance', title: '3.2 Key Mechanisms & ADHD Relevance' },
    { path: '/darm/table-4-foods-nutrients', title: 'Table 4: Foods & Nutrients for Microbiota/SCFA Balance + Neurotransmitter Precursors' },
    { path: '/darm/table-5-age-stratified-plans', title: 'Table 5: Age-Stratified Stepwise Plans with Decision Points' },
    { path: '/darm/table-5-age-stratified-plans/expected-recovery-timeline-early-intervention-0-6-years', title: 'Expected Recovery Timeline — Early Intervention (0–6 Years)' },
    { path: '/darm/table-5-age-stratified-plans/expected-recovery-timeline-later-intervention-6-years-with-medication', title: 'Expected Recovery Timeline — Later Intervention (6+ Years, with Medication)' },
    { path: '/darm/table-5-age-stratified-plans/phase-0-prenatal-maternal', title: 'Phase 0: Prenatal (Maternal)' },
    { path: '/darm/table-5-age-stratified-plans/phase-1-birth-6-months-critical-window', title: 'Phase 1: Birth–6 Months (Critical Window)' },
    { path: '/darm/table-5-age-stratified-plans/phase-2-6-months-2-years-diversification', title: 'Phase 2: 6 Months–2 Years (Diversification)' },
    { path: '/darm/table-5-age-stratified-plans/phase-3-2-6-years-pattern-establishment', title: 'Phase 3: 2–6 Years (Pattern Establishment)' },
    { path: '/darm/table-5-age-stratified-plans/phase-4-6-years-school-age-adhd', title: 'Phase 4: 6+ Years (School-Age ADHD)' }
  ],
  edges: [
    { from: '/darm', to: '/darm/adhs-darmbakterien-phaenotyp' },
    { from: '/darm', to: '/darm/detaillierte-bakterienprofile' },
    { from: '/darm', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm', to: '/darm/key-prevention-principles' },
    { from: '/darm', to: '/darm/probiotika-im-vergleich-tabelle' },
    { from: '/darm', to: '/darm/quellen-referenzen' },
    { from: '/darm', to: '/darm/table-0-dysbiotic-shifts-within-phyla' },
    { from: '/darm', to: '/darm/table-1-bacterial-dysbiosis-summary' },
    { from: '/darm', to: '/darm/table-2-clinical-monitoring-biomarkers' },
    { from: '/darm', to: '/darm/table-3-scfa-dysbalance' },
    { from: '/darm', to: '/darm/table-4-foods-nutrients' },
    { from: '/darm', to: '/darm/table-5-age-stratified-plans' },
    { from: '/darm/detaillierte-bakterienprofile', to: '/darm' },
    { from: '/darm/detaillierte-bakterienprofile/erhoehte-bakterienarten', to: '/darm/detaillierte-bakterienprofile' },
    { from: '/darm/detaillierte-bakterienprofile/reduzierte-bakterienarten', to: '/darm/quellen-referenzen' },
    { from: '/darm/detaillierte-interventionsstrategien', to: '/darm' },
    { from: '/darm/detaillierte-interventionsstrategien/4-1-prenatal-optimization-maternal-foundation', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm/detaillierte-interventionsstrategien/4-2-infancy-birth-to-6-months-critical-neonatal-colonization', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm/detaillierte-interventionsstrategien/4-3-toddlerhood-6-months-to-2-years-microbiota-diversification', to: '/darm/quellen-referenzen' },
    { from: '/darm/detaillierte-interventionsstrategien/4-4-early-childhood-2-to-6-years-dietary-pattern-establishment', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm/detaillierte-interventionsstrategien/4-5-school-age-child-6-years-two-pathways', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm/detaillierte-interventionsstrategien/4-6-medication-effects-on-microbiota-counteractive-strategies', to: '/darm/quellen-referenzen' },
    { from: '/darm/detaillierte-interventionsstrategien/4-7-antibiotic-exposure-recovery-protocol', to: '/darm/quellen-referenzen' },
    { from: '/darm/detaillierte-interventionsstrategien/critical-window-insight', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm/key-prevention-principles', to: '/darm' },
    { from: '/darm/probiotika-im-vergleich-tabelle', to: '/darm' },
    { from: '/darm/quellen-referenzen', to: '/darm' },
    { from: '/darm/table-1-bacterial-dysbiosis-summary', to: '/darm' },
    { from: '/darm/table-2-clinical-monitoring-biomarkers', to: '/darm' },
    { from: '/darm/table-2-clinical-monitoring-biomarkers/2-1-microbiota-composition-16s-rrna-sequencing', to: '/darm/table-2-clinical-monitoring-biomarkers' },
    { from: '/darm/table-2-clinical-monitoring-biomarkers/2-2-scfa-targets-fecal-gc-ms-plasma', to: '/darm/table-2-clinical-monitoring-biomarkers' },
    { from: '/darm/table-2-clinical-monitoring-biomarkers/2-3-symptom-functional-endpoints', to: '/darm/table-2-clinical-monitoring-biomarkers' },
    { from: '/darm/table-3-scfa-dysbalance', to: '/darm' },
    { from: '/darm/table-3-scfa-dysbalance', to: '/darm/quellen-referenzen' },
    { from: '/darm/table-3-scfa-dysbalance/3-1-scfa-levels-producers-neurotransmitter-impact', to: '/darm/table-3-scfa-dysbalance' },
    { from: '/darm/table-3-scfa-dysbalance/3-2-key-mechanisms-adhd-relevance', to: '/darm/quellen-referenzen' },
    { from: '/darm/table-4-foods-nutrients', to: '/darm' },
    { from: '/darm/table-5-age-stratified-plans', to: '/darm' },
    { from: '/darm/table-5-age-stratified-plans/expected-recovery-timeline-early-intervention-0-6-years', to: '/darm/table-5-age-stratified-plans' },
    { from: '/darm/table-5-age-stratified-plans/expected-recovery-timeline-later-intervention-6-years-with-medication', to: '/darm/table-5-age-stratified-plans' },
    { from: '/darm/table-5-age-stratified-plans/phase-0-prenatal-maternal', to: '/darm/table-5-age-stratified-plans' },
    { from: '/darm/table-5-age-stratified-plans/phase-1-birth-6-months-critical-window', to: '/darm/table-5-age-stratified-plans' },
    { from: '/darm/table-5-age-stratified-plans/phase-2-6-months-2-years-diversification', to: '/darm/table-5-age-stratified-plans' },
    { from: '/darm/table-5-age-stratified-plans/phase-3-2-6-years-pattern-establishment', to: '/darm/detaillierte-interventionsstrategien' },
    { from: '/darm/table-5-age-stratified-plans/phase-4-6-years-school-age-adhd', to: '/darm/table-5-age-stratified-plans' },
  ]
};
