# 0011 — What a topic discloses

**Status:** Accepted (2026-09-01)

## Context

[D-4](../superpowers/specs/2026-08-15-links-topics-tasks-design.md) decided that topics are
**not** nodes in the graph, and stated the consequence as a requirement: topics are invisible
in the graph, so browsing by topic needs its own view — *"a topic page listing its documents.
That is the ONLY way topics are reachable."*

Every aggregate view in this system is a disclosure surface, and the rule for all of them is
the same: filter **per document**, through the permission-checked accessor a page read uses,
never once per subtree. `graph_for` does it, `board_for` does it, backlinks do it.

**Topics add a second leak those do not have: a topic's own name.**

A board card's title is the page's words, and the card is only shown once the page has been
checked. A backlinks panel has no strings of its own — every name it prints belongs to a page
it has already authorised. A topic is different. `Kündigung Mietvertrag` is a name somebody
typed, stored in a table of its own, and it says something *without any document attached to
it at all*. Listing it to a reader who may see none of its pages tells them such a page
exists and roughly what it is about. Filtering the documents correctly and still printing the
name would leak exactly the thing the pages were restricted to protect.

Free text makes this worse rather than better: there is no vocabulary to review, and the
topics most worth hiding are the ones somebody typed in a hurry on the page that needed
hiding.

## Decision

**A topic exists, for a given caller, exactly when that caller may read at least one document
filed under it or under a topic inside it.**

A topic they can see no document of is:

- not listed in the index
- not counted
- not offered as a suggestion while typing
- answered as `None` when asked about by name — the same answer as a topic nobody ever typed

The last one is the point. The refusal and the absence must be **the same answer**, or the
difference between them is the oracle: ask for `/kündigung-mietvertrag`, get "forbidden"
rather than "no such topic", and the name is confirmed.

Descendants count towards this, because [`Store::topics_for`]'s listing is inclusive of
descendants for reasons recorded in `gw-store/src/topics.rs` — a topic whose only readable
pages sit two levels down is still a topic you can reach something through.

### The table is pruned, not just the view

A topic no page carries is **deleted**. So "this topic exists" and "this topic has at least
one document" are the same statement about the table as well as about a caller.

This is not tidiness. Without it, a name typed once on a page that was later retagged or
deleted would sit in the table forever — unreachable, uncounted, and still a name somebody
chose. Pruning means the only names that persist are ones a live page is currently asserting.

## Disclosure

**The residual channel is a count, and it is bounded by what the caller can already read.**
A caller who may read one page under `Medizin` learns that `Medizin` exists and how many
documents *they* can see in it. They learn nothing about the ones they cannot: the count is
computed from the same filtered set the list is, so it cannot be differenced against a total
that is never published.

**What this does not protect against**, stated plainly because a future reader will ask: a
caller who may read one page under a topic learns that topic's name, and can infer that other
people's pages may share it. That is inherent to a shared vocabulary — it is the same
information a shared folder name carries — and the alternative is per-caller topic
namespaces, which would make topics useless for the thing they exist for.

## Consequences

- **Every topic query is permission-shaped**, including the ones that look like metadata.
  There is no cheap "just list the tags" path, and there must not be one: it would be the
  second answer, and the second answer is the one that leaks.
- **A suggestion list is a disclosure surface.** The autocomplete that makes free text
  workable is filtered exactly like the index. This is easy to forget precisely because it
  feels like a UI convenience.
- **Counts cost a permission pass.** Accepted: the corpus is tens of pages, and the
  alternative is a number that means something different from the list beside it.

## Switch-back criteria

Revisit if either becomes true:

- **A managed vocabulary replaces free text.** If topics must be created deliberately by
  somebody trusted, then the name is no longer evidence about a page — it is a curated term,
  and listing the vocabulary to everyone becomes reasonable. The decision to make topics free
  text is what makes this one necessary.
- **The corpus grows enough that per-caller counting is measurably slow.** The fix is then a
  cache keyed by (caller, topic), not a cheaper answer — and if anybody proposes an unfiltered
  count to make a page fast, this document is the reason not to.
