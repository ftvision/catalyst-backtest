"""Client behavior against a mocked transport: lifecycle, polling, errors."""

from __future__ import annotations

import httpx
import pytest

from catalyst_client.api import ApiError, BacktestFailed, CatalystClient


def _client(handler) -> CatalystClient:
    return CatalystClient("http://test", transport=httpx.MockTransport(handler))


def test_submit_returns_id() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/backtests"
        return httpx.Response(202, json={"id": "run_1", "status": "queued"})

    with _client(handler) as client:
        assert client.submit({"graph": {}, "config": {}}) == "run_1"


def test_run_polls_to_completion_then_fetches_result() -> None:
    statuses = iter(["running", "succeeded"])

    def handler(request: httpx.Request) -> httpx.Response:
        path = request.url.path
        if path == "/backtests":
            return httpx.Response(202, json={"id": "run_1", "status": "queued"})
        if path == "/backtests/run_1":
            return httpx.Response(200, json={"id": "run_1", "status": next(statuses)})
        if path == "/backtests/run_1/result":
            return httpx.Response(200, json={"summary": {"final_value_usd": "123"}})
        raise AssertionError(path)

    with _client(handler) as client:
        result = client.run({"graph": {}}, poll_interval=0.0, timeout=5.0)
    assert result["summary"]["final_value_usd"] == "123"


def test_failed_run_raises_backtest_failed() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        if request.url.path == "/backtests":
            return httpx.Response(202, json={"id": "run_1"})
        return httpx.Response(200, json={"id": "run_1", "status": "failed", "error": "boom"})

    with _client(handler) as client:
        run_id = client.submit({})
        with pytest.raises(BacktestFailed) as exc:
            client.wait(run_id, poll_interval=0.0, timeout=5.0)
    assert "boom" in str(exc.value)


def test_error_envelope_is_parsed() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(
            400,
            json={"error": {"code": "invalid_request", "message": "bad graph"}, "hint": "x"},
        )

    with _client(handler) as client:
        with pytest.raises(ApiError) as exc:
            client.catalog()
    assert exc.value.status_code == 400
    assert exc.value.code == "invalid_request"
    assert exc.value.message == "bad graph"
    assert exc.value.extra == {"hint": "x"}


def test_wait_times_out() -> None:
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"id": "run_1", "status": "running"})

    with _client(handler) as client:
        with pytest.raises(TimeoutError):
            client.wait("run_1", poll_interval=0.0, timeout=0.0)
