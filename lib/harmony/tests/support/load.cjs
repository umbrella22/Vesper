'use strict';
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const ts = require(process.env.VESPER_HARMONY_TYPESCRIPT);

/** Each contract suite supplies explicit platform doubles and isolated modules. */
exports.createLoader = external => {
  const modules = new Map();
  const load = file => {
    if (modules.has(file)) return modules.get(file).exports;
    const module = { exports: {} }; modules.set(file, module);
    const code = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
      compilerOptions: { target: ts.ScriptTarget.ES2020, module: ts.ModuleKind.CommonJS },
      fileName: file.replace(/\.ets$/, '.ts'),
    }).outputText;
    const requireLocal = name => name.startsWith('.') ? load(path.resolve(path.dirname(file), name + '.ets')) : external(name);
    vm.runInThisContext('(function(require,module,exports){' + code + '\n})', { filename: file })(requireLocal, module, module.exports);
    return module.exports;
  };
  return load;
};
