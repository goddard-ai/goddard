CREATE TABLE `loops` (
	`id` text PRIMARY KEY,
	`createdAt` integer NOT NULL,
	`updatedAt` integer NOT NULL
);
--> statement-breakpoint
CREATE TABLE `sessions` (
	`id` text PRIMARY KEY,
	`status` text DEFAULT 'idle' NOT NULL,
	`agentName` text NOT NULL,
	`cwd` text NOT NULL,
	`mcpServers` text NOT NULL,
	`createdAt` integer NOT NULL,
	`updatedAt` integer NOT NULL,
	`errorMessage` text,
	`blockedReason` text,
	`initiative` text,
	`lastAgentMessage` text,
	`serverId` text UNIQUE,
	`serverAddress` text,
	`serverPid` integer,
	`metadata` text
);
