const emit = (event) => process.stdout.write(`##zen-test ${JSON.stringify(event)}\n`);

const STATUS = { passed: 'passed', pass: 'passed', failed: 'failed', fail: 'failed' };

const names = (testCase) => {
  const list = [testCase.name];
  for (let parent = testCase.parent; parent && parent.type === 'suite'; parent = parent.parent) {
    list.unshift(parent.name);
  }
  return list;
};

const failure = (file, test, errors) => {
  if (errors && errors.length > 0) {
    emit({ event: 'output', file, test });
    for (const error of errors) {
      process.stdout.write(`${error.stack ?? error.message ?? error}\n`);
    }
    emit({ event: 'outputEnd' });
  }
};

const finishLegacy = (task, file, parents) => {
  const test = [...parents, task.name];
  if (task.tasks) {
    task.tasks.forEach((child) => finishLegacy(child, file, test));
    return;
  }
  const state = task.result?.state;
  emit({ event: 'testStarted', file, test });
  failure(file, test, task.result?.errors);
  emit({ event: 'testFinished', file, test, status: STATUS[state] ?? 'skipped', durationMs: task.result?.duration ?? null });
};

export default class ZenReporter {
  seen = false;

  onTestModuleStart(module) {
    this.seen = true;
    emit({ event: 'fileStarted', file: module.moduleId });
  }

  onTestCaseReady(testCase) {
    emit({ event: 'testStarted', file: testCase.module.moduleId, test: names(testCase) });
  }

  onTestCaseResult(testCase) {
    const file = testCase.module.moduleId;
    const test = names(testCase);
    const result = testCase.result();
    failure(file, test, result.errors);
    emit({
      event: 'testFinished',
      file,
      test,
      status: STATUS[result.state] ?? 'skipped',
      durationMs: testCase.diagnostic()?.duration ?? null,
    });
  }

  onTestModuleEnd(module) {
    const state = module.state();
    failure(module.moduleId, null, module.errors?.());
    emit({
      event: 'fileFinished',
      file: module.moduleId,
      status: state === 'failed' ? 'failed' : 'passed',
      durationMs: module.diagnostic()?.duration ?? null,
    });
  }

  onFinished(files = []) {
    if (this.seen) {
      return;
    }
    for (const file of files) {
      emit({ event: 'fileStarted', file: file.filepath });
      file.tasks.forEach((task) => finishLegacy(task, file.filepath, []));
      failure(file.filepath, null, file.result?.errors);
      const status = file.result?.state === 'fail' ? 'failed' : 'passed';
      emit({ event: 'fileFinished', file: file.filepath, status, durationMs: file.result?.duration ?? null });
    }
  }
}
