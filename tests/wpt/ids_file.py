"""The per-test half of a corpus-run snapshot: `docs/wpt/runs/<name>.ids.tsv.gz`.

`run_corpus.py --run-json X.json` writes the aggregates to `X.json` and one row
per scored manifest id to `X.ids.tsv.gz`; `run_diff.py` and
`scripts/quality_report.py` read it back. The format lives here so that the
writer and both readers cannot drift apart, and so that the readers do not
have to import the runner (and with it `wptrunner`'s environment).

A row is tab-separated, sorted by id, with a header line:

    id  type  status  passed  total  score  shard

* `status` is the wptreport status (`OK`/`PASS`/`FAIL`/`TIMEOUT`/`ERROR`/
  `CRASH`/`SKIP`/...), or `NOT-RUN` (no verdict from anywhere), or `DROPPED`
  (wptrunner announced the test and threw it away, `run_corpus.dropped_from_raw_log`);
* `passed`/`total` are the subtests that passed and that were reported;
* `score` is exactly what `score_reports` added to the category — the file
  never recomputes it, so it sums to the snapshot's own `scored.totals.score`;
* `shard` is the shard (`state.json` name) that carried the id, or empty.

The gzip header carries no timestamp: a file written twice from the same rows is
byte-identical, which keeps a re-aggregation from showing up as a diff.
"""

import gzip
import io

COLUMNS = ("id", "type", "status", "passed", "total", "score", "shard")
SUFFIX = ".ids.tsv.gz"


def ids_path_for(run_json: str) -> str:
    """`runs/X.json` -> `runs/X.ids.tsv.gz`."""
    base = run_json[:-5] if run_json.endswith(".json") else run_json
    return base + SUFFIX


def _score_text(score: float) -> str:
    # `score_reports` adds the raw float; four digits is the resolution the
    # category pass-rate itself is rounded to, and keeps the file small.
    text = f"{score:.4f}".rstrip("0").rstrip(".")
    return text or "0"


def write(path: str, rows) -> int:
    """Write `rows` (`(id, type, status, passed, total, score, shard)` tuples) to
    `path`, sorted by id. Returns the size of the written file in bytes."""
    lines = ["\t".join(COLUMNS)]
    for test_id, test_type, status, passed, total, score, shard in sorted(rows):
        lines.append("\t".join((test_id, test_type, status, str(passed), str(total),
                                _score_text(score), shard or "")))
    payload = ("\n".join(lines) + "\n").encode("utf-8")
    with open(path, "wb") as raw:
        # mtime=0 and no filename: byte-identical output for identical rows.
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, compresslevel=9, mtime=0) as gz:
            gz.write(payload)
    with open(path, "rb") as fh:
        return len(fh.read())


def read(path: str) -> dict:
    """`{id: {"type", "status", "passed", "total", "score", "shard"}}`."""
    result = {}
    with gzip.open(path, "rb") as raw:
        text = io.TextIOWrapper(raw, encoding="utf-8", newline="\n")
        header = text.readline().rstrip("\n").split("\t")
        if tuple(header) != COLUMNS:
            raise ValueError(f"{path}: unexpected columns {header}, expected {list(COLUMNS)}")
        for line in text:
            line = line.rstrip("\n")
            if not line:
                continue
            test_id, test_type, status, passed, total, score, shard = line.split("\t")
            result[test_id] = {"type": test_type, "status": status, "passed": int(passed),
                               "total": int(total), "score": float(score), "shard": shard}
    return result
