# English, as America writes it.
#
# An overlay rather than a catalog: every message not written here is answered
# out of `en-GB.ftl`, which is the English this application is written in.
# Only what the two Englishes really disagree about belongs here, and
# `Catalog::validate` fails a file that copies a message across unchanged or
# writes one no other catalog has. See docs/localization.md.

# The month goes first.
release-date = { $month } { $day }, { $year }

# What flatpak keeps on this disk is a catalog with no -ue.
shelf-catalogue-note = The catalog every remote keeps on this disk.
catalogue-could-not-be-read = The catalog could not be read.
fetch-catalogue = Fetch catalog
catalogue-is-malformed = is not the catalog it should be: { $why }
fetching-catalogue = Fetching { $name }'s catalog
catalogue-could-not-be-fetched = { $name }'s catalog could not be fetched: { $why }
catalogue-is-up-to-date = { $name }'s catalog is up to date.

repository-added-for-this-user = Added for this user, so nothing has to be authorized.
