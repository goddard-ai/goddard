import type {
  PrCreateInput,
  PrReplyInput,
  ReplyPrDaemonRequest,
  SubmitPrDaemonRequest,
} from "./types.ts"
export declare function resolveSubmitRequestFromGit(
  input: SubmitPrDaemonRequest,
): Promise<PrCreateInput>
export declare function resolveReplyRequestFromGit(
  input: ReplyPrDaemonRequest,
): Promise<PrReplyInput>
//# sourceMappingURL=git.d.ts.map
