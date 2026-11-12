Synthetic structural encoding corpus. No patient data or partner exports.

The eighteen message fixtures cover ORU^R01, ORM^O01, OML^O21, ORL^O22,
ADT^A08 and ACK at each of v2.3, v2.4 and v2.5.1. They exercise repetitions,
components/subcomponents, escapes, empty fields, trailing delimiters, and ZLX.
OML/ORL version combinations are intentional generic parser tests, not claims
that a message type or segment is defined in every declared version. Tests in
codec.rs add custom delimiters, LF/CRLF, binary encodings and unknown local escapes.

Malformed fixtures cover a non-MSH first segment, missing MSH-2, truncated
header/escape, duplicate delimiters, wrong segment field separator, raw control
bytes and invalid hexadecimal escapes. They must return typed errors.
