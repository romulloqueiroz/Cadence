# Test fixtures

`Kontakt680Template.nki` is an empty Kontakt 6/8-format instrument container
from the open-source [ConvertWithMoss](https://github.com/git-moss/ConvertWithMoss)
project (LGPL-3.0). It is included here only as an input for format tests.

It exercises the NI container parser, FastLZ-compressed subtree decoding, and
the preset-chunk tree. It contains no samples and no user content.

`plain-playback.xml` is independently authored test data for the playable K2 XML
mapping. It exercises sparse group IDs, explicit release flags, encoded Unicode
sample paths and parameter composition. It contains no vendor preset or sample
content. Tests generate their own sample WAVs and compressed instrument headers.
