# Actual Java SQL highlighting

Thirty-two cases call the unchanged original `IdeaCodeHighlighter.tokenize`
using `SqlHighlightGolden.java`. The oracle runs without a GUI, database or
clipboard access. It records token text and palette categories, including
unterminated strings/comments, exponent/suffixed numbers, Unicode boundaries,
annotations and the original handling of functions separated by a newline.

Original source SHA-256:
`391A2659BB9FEC2E3323778FAF2024A345352CBA36C2442F3016D2F512DE7561`.
Fixture SHA-256:
`A708CD55DAF4B5348045AB79E57B1A8E455E92FB6CDCB8DD5D8581645EACC331`.
