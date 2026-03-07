import { JSONRPCClient } from "json-rpc-2.0"
import WebSocket from "ws"

export interface ClientOptions {
	url?: string
	refreshRateMs?: number
}

export function startClient(options: ClientOptions = {}) {
	const url = options.url || "ws://localhost:3000"
	const refreshRateMs = options.refreshRateMs || 100

	const ws = new WebSocket(url)

	let idCounter = 0
	const rpcClient = new JSONRPCClient(
		(request) => {
			if (ws.readyState === WebSocket.OPEN) {
				ws.send(JSON.stringify(request))
				return Promise.resolve()
			}
			return Promise.reject(new Error("WebSocket is not open"))
		},
		() => {
			idCounter++
			return idCounter
		},
	)

	ws.on("message", (data) => {
		rpcClient.receive(JSON.parse(data.toString()))
	})

	ws.on("open", () => {
		// 1. Set your local terminal to "raw mode" to capture keystrokes directly
		process.stdin.setRawMode(true)
		process.stdin.resume()
		process.stdin.setEncoding("utf8")

		// 2. Capture keystrokes and send to the server
		process.stdin.on("data", (key: string) => {
			// Exit cleanly if you press Ctrl+C
			if (key === "\u0003") {
				process.stdout.write("\x1B[2J\x1B[0f") // Clear screen on exit
				process.exit()
			}

			// Send the raw keystroke over JSON-RPC
			rpcClient.notify("rpc_write_input", { key })
		})

		// 3. Constantly update the local screen
		setInterval(async () => {
			try {
				// Ask the server for the current screen state
				const screenLines = (await rpcClient.request(
					"rpc_get_screen_state",
					{},
				)) as string[]

				// Clear your local terminal screen (using ANSI escape codes)
				process.stdout.write("\x1B[2J\x1B[0f")

				// Print the clean frame you got from the server
				process.stdout.write(screenLines.join("\n"))
			} catch (e) {
				// Handle fetch errors quietly during interval
			}
		}, refreshRateMs) // Pull updates periodically
	})

	ws.on("close", () => {
		console.log("Disconnected from server")
		process.exit(0)
	})

	ws.on("error", (error) => {
		console.error("WebSocket error:", error)
		process.exit(1)
	})

	return {
		ws,
		rpcClient,
		close: () => {
			ws.close()
		},
	}
}
