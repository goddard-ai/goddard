CREATE TABLE IF NOT EXISTS `sessions` (
	`id` text PRIMARY KEY,
	`acpId` text NOT NULL UNIQUE,
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
	`repository` text,
	`prNumber` integer,
	`metadata` text
);
--> statement-breakpoint
CREATE TABLE IF NOT EXISTS `loops` (
	`id` text PRIMARY KEY,
	`agent` text NOT NULL,
	`systemPrompt` text NOT NULL,
	`strategy` text,
	`displayName` text NOT NULL,
	`cwd` text NOT NULL,
	`mcpServers` text NOT NULL,
	`gitRemote` text DEFAULT 'origin' NOT NULL,
	`createdAt` integer NOT NULL,
	`updatedAt` integer NOT NULL
);
--> statement-breakpoint
CREATE TABLE IF NOT EXISTS `artifacts` (
	`id` text PRIMARY KEY,
	`sessionId` text NOT NULL,
	`type` text NOT NULL,
	`metadata` text,
	CONSTRAINT `fk_artifacts_sessionId_sessions_id_fk` FOREIGN KEY (`sessionId`) REFERENCES `sessions`(`id`)
);
--> statement-breakpoint
CREATE INDEX IF NOT EXISTS `sessions_repository_idx` ON `sessions` (`repository`);
--> statement-breakpoint
CREATE INDEX IF NOT EXISTS `sessions_repository_pr_number_idx` ON `sessions` (`repository`,`prNumber`);
