import { runAgent, providers } from "./src/runAgent";

async function main() {
  console.log("Starting manual integration test...");

  // We mock fetch so we can test the loop without real API calls
  const originalFetch = global.fetch;
  let pollCount = 0;

  global.fetch = async (url: RequestInfo | URL, init?: RequestInit) => {
    const urlStr = url.toString();
    console.log(`[Mock Fetch] ${init?.method || "GET"} ${urlStr}`);

    if (init?.method === "POST" && urlStr.includes("/agents")) {
      return new Response(JSON.stringify({ id: "mock-job-123" }));
    }

    if (urlStr.includes("/agents/mock-job-123/result")) {
      return new Response(JSON.stringify({
        success: true,
        summary: "Added rate limiting middleware",
        patch: "diff --git a/index.js b/index.js\n..."
      }));
    }

    if (urlStr.includes("/agents/mock-job-123")) {
      pollCount++;
      // Return 'running' first time, then 'completed'
      return new Response(JSON.stringify({
        status: pollCount > 1 ? "completed" : "running"
      }));
    }

    return new Response("{}");
  };

  try {
    const provider = providers["cursor-cloud"];

    // Override setTimeout to run faster in tests
    const originalSetTimeout = global.setTimeout;
    (global as any).setTimeout = (cb: any, ms: number) => {
      return originalSetTimeout(cb, 100); // 100ms instead of 5000ms
    };

    const result = await runAgent(provider, {
      prompt: "add rate limiting middleware",
      repo: {
        type: "github",
        owner: "acme",
        repo: "api"
      }
    });

    console.log("\nJob finished!");
    console.log("Result:", result);

    // Restore
    global.fetch = originalFetch;
    global.setTimeout = originalSetTimeout;
  } catch (err) {
    console.error("Test failed:", err);
  }
}

main().catch(console.error);
