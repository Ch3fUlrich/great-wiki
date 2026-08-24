### Changed

- **A card says who has it by name.** The board showed the account id, because the API
  answered one and resolving a name would have been a lookup per card; it resolves one now,
  once per distinct account, and the card shows it.
  The id has not gone away, and the case it covers is the point: **a card can name somebody
  this reader may not be told about** — the account may no longer read the page the card is
  governed by, or may be suspended — and then the id is what is shown, exactly as before.
  That is not an error state and it is not a missing account, so there is no »Unbekannt« and
  the row is not hidden: "somebody has this" is the fact that matters on a board, and the id
  is the handle whoever wants to clear it would type.

> Written from the interface side. The API half of this change is not described here; fold
> this into whichever entry covers it, or keep it if there is none.
