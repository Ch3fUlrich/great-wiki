### Fixed

- Page creation: a slug is capped at 100 characters (truncate on char boundary, then trim trailing '-') and a slug of only punctuation is refused, matching the title's length and empty-content rules.
- Move operation: a page in the trash still holds its address, so moving to an occupied address that is trashed says "a page in the trash still holds {path}: restore or purge it first" rather than "there is already a page at {path}".