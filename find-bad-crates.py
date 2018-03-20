import os
import json
import glob

RESULT_PATHS = 'result_paths.json'
RESULTS = 'results.json'

if not os.path.exists(RESULT_PATHS):
    # starts are: ex, commitish, reg/gh, crate
    result_paths = glob.glob('work/ex/*/res/*/*/*/results.txt')
    with open(RESULT_PATHS, 'w') as f:
        f.write(json.dumps(result_paths))
with open(RESULT_PATHS) as f:
    result_paths = json.load(f)

if not os.path.exists(RESULTS):
    results = {}
    for path in result_paths:
        with open(path) as f:
            pathdir = path[:-(len('results.txt')+1)]
            results[pathdir] = f.read()
    with open(RESULTS, 'w') as f:
        f.write(json.dumps(results))
with open(RESULTS) as f:
    results = json.load(f)

sames = {}
for pathdir, result in results.iteritems():
    assert result in ['build-fail', 'test-fail', 'test-pass', 'test-skipped']
    head, crate = os.path.split(pathdir)
    _, gh_or_reg = os.path.split(head)
    if gh_or_reg == 'gh':
        # e.g. pepyakin.chipster.9cd0c41e16b8e1a58381b3fb18ed412c92237f4e
        name = crate.rsplit('.', 1)[0]
    elif gh_or_reg == 'reg':
        # e.g. effect-monad-0.3.1
        name = crate.rsplit('-', 1)[0]
    else:
        assert False, pathdir

    key = (gh_or_reg, name)
    cur = sames.get(key)
    if cur is None:
        sames[key] = (result, 1)
    else:
        cur_result, cur_count = cur
        if cur_result is None:
            # This one has already mismatched
            pass
        elif result != cur_result:
            sames[key] = (None, 0)
        else:
            sames[key] = (cur_result, cur_count + 1)

num_exs = len(glob.glob('work/ex/*'))
# Can be more than this, crater sometimes decides to test multiple versions of a crate
all_fails = 2 * num_exs

ghlines = []
reglines = []
for (gh_or_reg, name), (result, count) in sames.iteritems():
    if result is None:
        continue
    if result in ['test-pass', 'test-skipped']:
        continue
    if count != all_fails:
        continue

    if result == 'build-fail':
        skipkind = 'skip'
    elif result == 'test-fail':
        skipkind = 'skip-tests'
    else:
        assert False, result
    if gh_or_reg == 'gh':
        if name.count('.') > 1:
            # Can't be bothered figuring this out - probably means the repo name has a dot
            continue
        org, repo = name.split('.')
        ghlines.append('"{}/{}" = {{ {} = true }}'.format(org, repo, skipkind))
    else:
        if name.count('.') > 1:
            # Can't be bothered figuring this out - probably means the crate version had
            # a -beta1 or something
            continue
        reglines.append('{} = {{ {} = true }}'.format(name, skipkind))
    #print '|'.join((gh_or_reg, name)), result, count
ghlines.sort()
reglines.sort()

open('ghlines.toml', 'w').write('\n'.join(ghlines))
open('reglines.toml', 'w').write('\n'.join(reglines))
