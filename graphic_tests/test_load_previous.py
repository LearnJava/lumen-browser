"""Регрессия BUG-1039: _load_previous не должен считать baselines.json прогоном."""
import json
import os
import sys
import tempfile

sys.path.insert(0, os.path.dirname(__file__))
import run


def _write(d, name, tag):
    with open(os.path.join(d, name), 'w', encoding='utf-8') as f:
        json.dump({'tag': tag}, f)


def test_skips_baselines_and_latest():
    with tempfile.TemporaryDirectory() as d:
        _write(d, 'baselines.json', 'baselines')
        _write(d, 'latest.json', 'latest')
        _write(d, '20260101-000000.json', 'previous')
        _write(d, '20260102-000000.json', 'current')
        assert run._load_previous(d)['tag'] == 'previous'


def test_single_run_has_no_previous():
    with tempfile.TemporaryDirectory() as d:
        _write(d, 'baselines.json', 'baselines')
        _write(d, '20260102-000000.json', 'current')
        assert run._load_previous(d) is None


if __name__ == '__main__':
    test_skips_baselines_and_latest()
    test_single_run_has_no_previous()
    print('OK')
