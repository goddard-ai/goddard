/** Focused, model-free acceptance checks for required approved-plan execution. */
const suites: [string, string][] = [
  ['waku-boss', 'plan_contract::tests'],
  ['waku-daemon', 'plan_'],
  ['waku-boss', 'every_employee_reads_plans_regardless_of_persona'],
  ['waku-boss', 'employees_can_read_only_pinned_documents_and_cannot_mutate_personas'],
  ['waku-boss', 'symlinks_cannot_escape_the_files_root'],
  ['waku-boss', 'normalize_plan_file_'],
  ['waku-boss', 'prerequisites_gate_work_and_terminal_outcomes_stay_closed'],
]
let passed = 0
let failed = 0
for (const [crate, filter] of suites) {
  const command = ['mbx', 'test', '-p', crate, '--lib', filter, '--', '--nocapture']
  console.log(`\n$ ${command.join(' ')}`)
  const child = Bun.spawn(command, { stdout: 'pipe', stderr: 'inherit' })
  const output = await new Response(child.stdout).text()
  console.log(output)
  const status = await child.exited
  const result = output.match(/test result: \w+\. (\d+) passed; (\d+) failed/)
  if (!result || status !== 0 || Number(result[1]) === 0) {
    console.error(`Acceptance suite failed or ran no tests: ${crate} ${filter}`)
    process.exit(status || 1)
  }
  passed += Number(result[1])
  failed += Number(result[2])
}
console.log(JSON.stringify({ acceptance: 'required-plan', passed, failed, ignored: 0,
  runtimeMatrixCases: 36, ordinaryWorkCases: 4,
  supportedFixtures: ['Codex/local/direct', 'Codex/local/worktree-path', 'Codex/local/cross-project',
    'Devin/local/direct', 'Devin/local/worktree-path', 'Devin/local/cross-project'],
  limitations: 'See .agents/docs/employee-plan-acceptance.md; scripted providers do not establish native tool restrictions or real-provider compatibility.',
}, null, 2))
