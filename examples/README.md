# Examples

Run `spacl init` before you use these examples. Start the coordinator and robot runtime in separate terminals.

- `curl/flow.sh`: issue and execute one token with `curl` and `jq`
- `httpie/flow.sh`: status, issue, and emergency-stop examples with HTTPie
- `python/planner.py`: two-step planner with the Python standard library
- `typescript/planner.ts`: two-step planner with native `fetch`
- `web/index.html`: static development status and metrics page

Serve the web example from its allowed development origin:

```bash
python3 -m http.server 8000 --directory examples/web
```

Then open `http://127.0.0.1:8000`.

These clients use plaintext development endpoints. Do not use them on a production robot network.
