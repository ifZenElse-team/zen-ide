'use strict';

const emit = (event) => process.stdout.write(`##zen-test ${JSON.stringify(event)}\n`);

const names = (result) => [...(result.ancestorTitles ?? []), result.title];

const STATUS = { passed: 'passed', failed: 'failed', pending: 'skipped', skipped: 'skipped', todo: 'skipped', disabled: 'skipped' };

class ZenReporter {
  constructor() {
    this.started = new Set();
    this.finished = new Set();
    this.reported = new Map();
  }

  fileStarted(path) {
    if (!this.started.has(path)) {
      this.started.add(path);
      this.reported.set(path, new Set());
      emit({ event: 'fileStarted', file: path });
    }
  }

  report(path, result) {
    const test = names(result);
    const key = test.join('\u0000');
    if (this.reported.get(path)?.has(key)) {
      return;
    }
    this.reported.get(path)?.add(key);
    const messages = result.failureMessages ?? [];
    if (messages.length > 0) {
      emit({ event: 'output', file: path, test });
      process.stdout.write(`${messages.join('\n')}\n`);
      emit({ event: 'outputEnd' });
    }
    const status = STATUS[result.status] ?? 'skipped';
    emit({ event: 'testFinished', file: path, test, status, durationMs: result.duration ?? null });
  }

  onTestStart(test) {
    this.fileStarted(test.path);
  }

  onTestFileStart(test) {
    this.fileStarted(test.path);
  }

  onTestCaseStart(test, info) {
    this.fileStarted(test.path);
    emit({ event: 'testStarted', file: test.path, test: names(info) });
  }

  onTestCaseResult(test, result) {
    this.fileStarted(test.path);
    this.report(test.path, result);
  }

  finishFile(test, results) {
    const path = test.path;
    if (this.finished.has(path)) {
      return;
    }
    this.finished.add(path);
    this.fileStarted(path);
    for (const result of results.testResults ?? []) {
      this.report(path, result);
    }
    if (results.failureMessage && results.testExecError) {
      emit({ event: 'output', file: path, test: null });
      process.stdout.write(`${results.failureMessage}\n`);
      emit({ event: 'outputEnd' });
    }
    const failed = results.numFailingTests > 0 || Boolean(results.testExecError);
    const durationMs = results.perfStats ? results.perfStats.end - results.perfStats.start : null;
    emit({ event: 'fileFinished', file: path, status: failed ? 'failed' : 'passed', durationMs });
  }

  onTestResult(test, results) {
    this.finishFile(test, results);
  }

  onTestFileResult(test, results) {
    this.finishFile(test, results);
  }
}

module.exports = ZenReporter;
