#!/usr/bin/env python3
"""End-to-end Studio test with Playwright.

Starts the real Anvil binary on a loopback port, drives the browser through
schematic placement, simulation, board generation, autoroute and Gerber preview,
and fails on console errors. Run from the repository root:

    python3 tests/studio_playwright.py
"""

import os
import re
import subprocess
import sys
import time
from pathlib import Path

from playwright.sync_api import sync_playwright


def binary() -> str:
    env = os.environ.get("ANVIL_BIN")
    if env:
        return env
    for candidate in ("target/release/anvil", "target/debug/anvil"):
        if Path(candidate).exists():
            return candidate
    return "target/debug/anvil"


def main() -> int:
    proc = subprocess.Popen(
        [binary(), "--no-browser", "--port", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )
    url = None
    try:
        deadline = time.time() + 20
        while time.time() < deadline:
            line = proc.stdout.readline()
            if not line:
                break
            match = re.search(r"http://\S+", line)
            if match:
                url = match.group(0)
                break
        if not url:
            print("FAIL: server did not print a URL")
            return 1

        errors = []
        with sync_playwright() as p:
            browser = p.chromium.launch()
            page = browser.new_page(viewport={"width": 1440, "height": 900})
            page.on(
                "console",
                lambda m: errors.append(f"console {m.type}: {m.text}")
                if m.type == "error"
                else None,
            )
            page.on("pageerror", lambda e: errors.append(f"pageerror: {e}"))
            page.goto(url, wait_until="networkidle")
            page.wait_for_timeout(800)

            assert page.inner_text("#project-name") == "Anvil demo"
            assert "clean" in page.inner_text("#net-status")
            assert page.locator("#schematic-canvas").is_visible()
            page.screenshot(path="/tmp/anvil_playwright_schematic.png")

            # Draw: pick a resistor and place it on an empty part of the sheet.
            before = page.inner_text("#schematic-hud")
            page.click('.part[data-part="R"]')
            box = page.locator("#schematic-canvas").bounding_box()
            page.mouse.dblclick(box["x"] + box["width"] * 0.72, box["y"] + box["height"] * 0.62)
            page.wait_for_timeout(600)
            after = page.inner_text("#schematic-hud")
            assert before != after, "placing a component did not update the sheet"

            # Restore the demo, then simulate.
            page.click("#load-demo")
            page.wait_for_timeout(900)
            page.click("#tab-simulate")
            page.click("#simulate-run")
            page.wait_for_timeout(1500)
            status = page.inner_text("#sim-status")
            assert "Transient" in status, status
            legend = page.inner_text("#legend")
            assert "MID" in legend, legend
            page.screenshot(path="/tmp/anvil_playwright_sim.png")

            # Board: generate, run DRC, autoroute.
            page.click("#tab-board")
            page.wait_for_timeout(400)
            page.click("#generate-board")
            page.wait_for_timeout(700)
            page.click("#run-drc")
            page.wait_for_timeout(500)
            assert "unrouted" in page.inner_text("#drc-summary").lower() or "clean" in page.inner_text("#drc-summary").lower()
            page.click("#autoroute")
            deadline = time.time() + 30
            while time.time() < deadline:
                if "clean" in page.inner_text("#drc-summary").lower():
                    break
                page.wait_for_timeout(200)
            else:
                raise AssertionError("autoroute did not reach a clean DRC")
            assert "routed clean" in page.inner_text("#route-status")
            assert "4/4" in page.inner_text("#route-log")
            page.screenshot(path="/tmp/anvil_playwright_board.png")

            # Export preview.
            page.click("#tab-export")
            page.wait_for_timeout(400)
            page.click("#preview-top")
            deadline = time.time() + 15
            while time.time() < deadline:
                src = page.get_attribute("#preview", "src") or ""
                if src.startswith("blob:") and page.locator("#preview").is_visible():
                    break
                page.wait_for_timeout(200)
            else:
                raise AssertionError("Gerber preview did not load")
            status = page.inner_text("#export-status")
            assert "PNG" in status, status
            page.screenshot(path="/tmp/anvil_playwright_export.png")

            browser.close()

        if errors:
            print("FAIL: console errors:")
            for e in errors:
                print("  " + e)
            return 1
        print("PLAYWRIGHT OK: draw, simulate, route, export preview")
        return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
