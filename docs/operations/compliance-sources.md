# Compliance sources

Sources cited by the [laboratory compliance guide](compliance.md). Every source was checked
on **2026-11-06**; the guide's statements about proposed rules and the current edition of any
standard are as of that date. "Verification" says how the cited text was obtained:

- **primary**: read from the publisher's own text (eCFR, Federal Register, agency PDF,
  standards body page).
- **primary (quoted)**: regulation text read as reproduced verbatim inside another primary
  source, such as CMS Appendix C quoting 42 CFR 493.
- **secondary**: known from a search summary, a law-firm or trade article, or an
  outdated public copy; confirm before relying on it.
- **unverified**: the fetch failed and the content comes from memory or a snippet.

IDs are cited in the guide as `[H1]`, `[C2]` and so on.

## HIPAA and HHS (H)

| ID | Source | Publisher | URL | Date / effective | Status | Verification and notes |
| --- | --- | --- | --- | --- | --- | --- |
| H1 | 45 CFR 164.312, Technical safeguards | eCFR (GPO/OFR) | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-C/section-164.312 | in force; eCFR version of 2026-11-01 | in force | primary |
| H2 | 45 CFR 164.306, General rules (addressable specifications in (d)) | eCFR | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-C/section-164.306 | same | in force | primary |
| H3 | 45 CFR 164.308, Administrative safeguards | eCFR | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-C/section-164.308 | same | in force | primary |
| H4 | 45 CFR 164.310, Physical safeguards (device and media controls (d)) | eCFR | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-C/section-164.310 | same | in force | primary |
| H5 | 45 CFR 164.316, Policies, procedures and documentation (6-year retention (b)(2)) | eCFR | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-C/section-164.316 | same | in force | primary |
| H6 | 45 CFR 164.402 and 164.404, Breach definitions and notice to individuals | eCFR | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-D | same | in force | primary |
| H7 | 45 CFR 164.502(b) and 164.514(b), (d), Minimum necessary and de-identification | eCFR | https://www.ecfr.gov/current/title-45/subtitle-A/subchapter-C/part-164/subpart-E | same | in force | primary for 164.502(b) and 164.514(b); 164.514(d) returned headings only |
| H9 | NPRM, "HIPAA Security Rule To Strengthen the Cybersecurity of Electronic Protected Health Information", 90 FR 898, FR Doc. 2024-30983, RIN 0945-AA22 | HHS/OCR, Federal Register | https://www.govinfo.gov/content/pkg/FR-2025-01-06/html/2024-30983.htm | published 2025-01-06; comments closed 2025-03-07 | **proposed** | primary for the preamble summary (required encryption, asset inventory, timing); the operative section was not read in full |
| H10 | Unified Agenda entry, RIN 0945-AA22 | OIRA, reginfo.gov | https://www.reginfo.gov/public/do/eAgendaViewRule?pubId=202510&RIN=0945-AA22 | agenda pubId 202510, publication ID "2026" | stage "Long-Term Actions"; final action "07/00/2027" | primary (re-fetched) |
| H11 | Federal Register documents for RIN 0945-AA22 | Federal Register API | https://www.federalregister.gov/api/v1/documents.json?conditions[term]=0945-AA22 | as of 2026-11-06 | only the 2025-01-06 NPRM; no final rule or withdrawal | primary |
| H13 | Guidance to Render Unsecured PHI Unusable, Unreadable, or Indecipherable to Unauthorized Individuals (74 FR 19006) | HHS | https://www.hhs.gov/hipaa/for-professionals/breach-notification/guidance/index.html | 2009-04-27 | guidance | **secondary** (HTTP 403): encryption per NIST SP 800-111 at rest and SP 800-52/800-77/800-113 in transit; destruction per SP 800-88 |
| H20 | NIST SP 800-66 Rev. 2, Implementing the HIPAA Security Rule: A Cybersecurity Resource Guide | NIST | https://nvlpubs.nist.gov/nistpubs/SpecialPublications/NIST.SP.800-66r2.pdf | final 2024-02-14 | guidance | primary (Tables 21 and 22, §5.1.1, §5.3.1, §5.3.2) |

## CLIA, accreditation and states (C)

| ID | Source | Publisher | URL | Date / effective | Status | Verification and notes |
| --- | --- | --- | --- | --- | --- | --- |
| C1 | 42 CFR Part 493 (§493.1105, .1251, .1283, .1291) | eCFR | https://www.ecfr.gov/current/title-42/chapter-IV/subchapter-G/part-493 | in force | in force | primary (quoted in C2); the eCFR fetch paraphrased |
| C2 | State Operations Manual, Appendix C: Survey Procedures and Interpretive Guidelines for Laboratories and Laboratory Services | CMS | https://www.cms.gov/regulations-and-guidance/guidance/manuals/downloads/som107ap_c_lab.pdf | tags Rev. 233 (2025-09-12); footer Rev. 236 (2026-01-23) | current | primary (full text read): D5801, D5805, D5819, D5821, D3027–D3043, D5789, D5403 |
| C3 | S&C-10-12-CLIA, Revised Survey Procedures and Interpretive Guidelines to Facilitate the Electronic Exchange of Laboratory Information | CMS | https://www.cms.gov/Medicare/Provider-Enrollment-and-Certification/SurveyCertificationGenInfo/downloads/scletter10-12.pdf | 2010-03-01 | archived (page expired 2025-12-04); content carried into C2 | primary |
| C4 | CAP Laboratory General Checklist | College of American Pathologists | public copy: https://www.illumina.com/content/dam/illumina-marketing/documents/services/laboratory-general-checklist.pdf | **edition 2016-08-17** | outdated edition | primary for the 2016 text only; the current edition is member-only and was not read |
| C14 | Cal. Bus. & Prof. Code §1265(j); 17 CCR §1053 | California (via public.law, Cornell LII) | https://california.public.law/codes/ca_bus_and_prof_code_section_1265 | current | in force | secondary |
| C15 | 10 NYCRR §58-1.11, Clinical laboratory records retention | New York (via Cornell LII) | https://www.law.cornell.edu/regulations/new-york/10-NYCRR-58-1.11 | current | in force; proposed shortening not confirmed | secondary |

## FDA (F)

| ID | Source | Publisher | URL | Date / effective | Status | Verification and notes |
| --- | --- | --- | --- | --- | --- | --- |
| F1 | FD&C Act §520(o), 21 U.S.C. 360j(o) (added by 21st Century Cures Act §3060) | US Code (GovInfo) | https://www.govinfo.gov/content/pkg/USCODE-2023-title21/html/USCODE-2023-title21-chap9-subchapV-partA-sec360j.htm | enacted 2016-12-13 | in force | (o)(1)(D) primary (quoted in F3); (o)(1)(E) **secondary** |
| F2 | Medical Device Data Systems, Medical Image Storage Devices, and Medical Image Communications Devices | FDA | https://www.fda.gov/media/88572/download | 2022-09-28 (first 2015-02-09) | final guidance | primary (full text) |
| F3 | Changes to Existing Medical Software Policies Resulting from Section 3060 of the 21st Century Cures Act | FDA | https://www.fda.gov/media/109622/download | 2019-09-27 | final guidance | primary (full text); §III.A on LIS/LIMS |
| F5 | 21 CFR 862.2100, Calculator/data processing module for clinical use | eCFR | https://www.ecfr.gov/current/title-21/chapter-I/subchapter-H/part-862/subpart-C/section-862.2100 | in force | Class I, 510(k)-exempt subject to 862.9 | primary |

## Limits of the weaker rows

- The current CAP Laboratory General checklist is member-only and was not read. Item numbers and
  text in the guide come from the 2016 edition (C4); confirm them with the accreditor.
- The HHS pages for H13 returned HTTP 403, so its content is secondary.
- Exact wording of FD&C Act §520(o)(1)(E) was not read from the US Code (F1 relies on
  (o)(1)(D), quoted in F3).
