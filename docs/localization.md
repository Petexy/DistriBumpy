# Languages

Software Hub speaks the ten languages LineXinBar does: **German**, **English**,
**Spanish**, **French**, **Hindi**, **Polish**, **Brazilian Portuguese**,
**Russian** and **Simplified Chinese** — nine catalogs, because the two
Englishes are one file here. All of them are compiled into the binary, and so
are the faces they are written in: there is nothing to install, no translation
package, no message directory and no font to find.

## Which language it speaks

The session's, decided once as it starts: the first of `LC_ALL`, `LC_MESSAGES`
and `LANG` that says anything names it, `pl_PL.UTF-8` and `pl-PL` alike are
Polish, `fr_CA` and `fr_BE` alike are French, `es_MX` is Spanish, `de_AT` is
German, `pt_PT` reads the Brazilian catalog and `zh_TW` the Simplified one,
and anything else — including `C`, `POSIX` and an empty environment — is
English. Nothing here writes to the environment or keeps a language setting of
its own.

On LineXinBar the shell exports the session's locale to everything it opens, so
this comes up in the language Settings > Language names. An application already
running keeps the language it started in, so a language changed there reaches
this one the next time it is opened.

To see a page in the other language without changing anything:

```sh
LC_ALL=pl_PL.UTF-8 distribumpy
LC_ALL=en_US.UTF-8 distribumpy
```

Those locales need not be generated on the machine: the catalogs are chosen by
the name, not by libc.

## Where the words are

* `locales/en-GB.ftl` and the eight translations beside it — everything this
  application says.
  `en-GB` is the English the toolkit falls back to, and there is no
  `en-US.ftl`: the two Englishes differ over the order of a date and a handful
  of spellings, and this application writes neither. An overlay holding nothing
  would fail `Catalog::validate`, which is the right answer — a file with no
  difference in it is a file to delete.
* `src/i18n.rs` — the catalogs it embeds, and `text` / `message!`.
* The file question, its menus and the notes under its rows are **the
  toolkit's** words, in `lxb-toolkit`'s own catalogs.

Names that belong to a file, a folder, a camera or a person are never
translated, and neither is anything another program said when it failed.

## What Flathub says

An application's name, summary, description, release notes, developer and
screenshot captions come from the remote's own AppStream catalogue, which
carries a translation of each of them per language (`xml:lang`). The one this
session speaks wins; where a remote has no translation of a field the
untranslated English one is used, whichever order the two are written in. That
is the publisher's text, not this application's, so it is not in `locales/` and
cannot be corrected here — an application with no Polish summary on Flathub is
shown with its English one.

Identifiers are never translated: the application id, the remote's name, the
runtime, the permission keys and everything else the machine reads.

## Adding a language

Copy `locales/en-GB.ftl` to `<tag>.ftl`, translate the values, and add it to
`RESOURCES` in `src/i18n.rs`; do the same in the toolkit, and add the tag to
`lxb_toolkit::i18n::LANGUAGES`. Translate `data/*.desktop` (`Name[xx]`,
`GenericName[xx]`, `Comment[xx]`, `Keywords[xx]`) and the AppStream metadata.
Keep every identifier and variable name as English has them, put the variables
in the order the language wants, and give it as many plural forms as it has.

A language written in a script the toolkit ships no face for needs one bundled
there first: `every_word_the_catalogs_carry_can_be_drawn` shapes every line of
these catalogs through the shipped faces alone and fails on the first
character none of them has — which is also what says a Chinese sentence has to
be reworded, since the Han face is cut to GB 2312.

The whole of it, including what to do about counts and dates, is in the
toolkit's [localization guide](../../lxb-toolkit/docs/localization.md) —
installed alongside it, and in the repository beside this one.

## Building and checking

```sh
cargo test           # the catalogs, and everything else
cargo fmt --check
```

Two of those tests are about the catalogs: that every language carries the same
messages with the same variables and that each of them formats for 0, 1, 2, 5,
12, 22 and 112, and that every identifier the sources ask for is one the
catalogs have. A test that compares a label against English text is a test that
fails on a Polish machine, so where a test needs the words it asks the catalog
for them the way the code does.

This revision needs the matching **lxb-toolkit** installed. The manifest reads
the toolkit's sources from `/usr/share/lxb-toolkit/crates`, so a machine
carrying an older release has no `i18n` module to compile against and says so:

```text
error[E0432]: unresolved import `lxb_toolkit::i18n`
```

and a package build stops earlier still, in `cargo fetch --locked`, because
`Cargo.lock` names dependencies that older toolkit does not have. Build and
install the toolkit's development package first — `lxb-toolkit-dev`, or
`-devel` on Fedora — and both go away.

`LXB_TOOLKIT_CRATE_DIR` does **not** do this. It tells the packaging scripts
where to *check* that the toolkit's sources exist; the manifest inside the
source archive still names `/usr/share/lxb-toolkit/crates`. To build against a
checkout without installing anything, point the paths in `Cargo.toml` at it by
hand and put them back afterwards — a path dependency records no location in
`Cargo.lock`, so the lock does not move.

Package builds embed the catalogs like any other build, so an installed
application needs nothing further.
