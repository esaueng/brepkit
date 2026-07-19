window.BENCHMARK_DATA = {
  "lastUpdate": 1784435358976,
  "repoUrl": "https://github.com/esaueng/brepkit",
  "entries": {
    "Boolean perf": [
      {
        "commit": {
          "author": {
            "email": "171875562+petergstfsn@users.noreply.github.com",
            "name": "Peter",
            "username": "petergstfsn"
          },
          "committer": {
            "email": "noreply@github.com",
            "name": "GitHub",
            "username": "web-flow"
          },
          "distinct": true,
          "id": "c27846057517ea0a1234acd6dcb0d65a59eb8bbc",
          "message": "Merge pull request #1 from esaueng/codex/production-hardening\n\nHarden brepkit for production",
          "timestamp": "2026-07-19T00:26:29-04:00",
          "tree_id": "22dcc21f601f2b220d8b0cc2c359813d93025912",
          "url": "https://github.com/esaueng/brepkit/commit/c27846057517ea0a1234acd6dcb0d65a59eb8bbc"
        },
        "date": 1784435358241,
        "tool": "cargo",
        "benches": [
          {
            "name": "boolean/cut_box_box",
            "value": 890041,
            "range": "± 3109",
            "unit": "ns/iter"
          },
          {
            "name": "boolean/fuse_box_box",
            "value": 978039,
            "range": "± 2209",
            "unit": "ns/iter"
          },
          {
            "name": "boolean/intersect_box_box",
            "value": 12589,
            "range": "± 72",
            "unit": "ns/iter"
          },
          {
            "name": "boolean/cut_cylinder_through_box",
            "value": 650616,
            "range": "± 635",
            "unit": "ns/iter"
          },
          {
            "name": "boolean/perforated_cut_36",
            "value": 26977374,
            "range": "± 110084",
            "unit": "ns/iter"
          }
        ]
      }
    ]
  }
}