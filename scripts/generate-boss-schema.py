#!/usr/bin/env python3
"""Refresh local Boss input contracts from generated protocol types and serde.

Run from the repository root after refreshing ts-rs protocol exports. --check
compares the checked-in definitions without executing any daemon operation.
Nullable TypeScript fields retain null support; serde controls input omission.
"""
import re, json, pathlib
root = pathlib.Path('packages/waku-client/src/generated')
defs = {}
refs = set()

def parse_type(s):
    s = s.strip()
    depth = 0
    parts = []
    start = 0
    for i, c in enumerate(s):
        if c in '{[<(':
            depth += 1
        if c in '}]>)':
            depth -= 1
        if c == '|' and depth == 0:
            parts.append(s[start:i])
            start = i + 1
    if parts:
        return {'anyOf': [parse_type(x) for x in parts + [s[start:]]]}
    if s.startswith('{ [key in string]'):
        return {'type': 'object', 'additionalProperties': parse_type(s.split(':', 1)[1].rsplit('}', 1)[0])}
    if s.startswith('{'):
        body = s[1:-1]
        fields = {}
        required = []
        depth = 0
        start = 0
        parts = []
        for i, c in enumerate(body):
            if c in '{[<(':
                depth += 1
            if c in '}]>)':
                depth -= 1
            if c in ',;' and depth == 0:
                parts.append(body[start:i])
                start = i + 1
        parts.append(body[start:])
        for p in parts:
            if not p.strip():
                continue
            m = re.match('\\s*("[^\\"]+"|\\w+)(\\?)?\\s*:\\s*(.*)', p, re.S)
            if not m:
                raise ValueError(p)
            k = m[1].strip('"')
            v = parse_type(m[3])
            fields[k] = v
            if not m[2]:
                required.append(k)
        return {'type': 'object', 'properties': fields, 'required': required, 'additionalProperties': False}
    if s.startswith('Array<'):
        return {'type': 'array', 'items': parse_type(s[6:-1])}
    if s.endswith('[]'):
        return {'type': 'array', 'items': parse_type(s[:-2])}
    if s.startswith('"'):
        return {'const': json.loads(s)}
    if s in ['string', 'boolean', 'null']:
        return {'type': s}
    if s == 'number':
        return {'type': 'integer'}
    if s == 'unknown' or s == 'any':
        return {}
    refs.add(s)
    return {'$ref': '#/definitions/' + s}

def load(name):
    raw = (root / (name + '.ts')).read_text()
    raw = re.sub('/\\*.*?\\*/', '', raw, flags=re.S)
    body = re.search('export type ' + name + '\\s*=\\s*(.*);', raw, re.S)[1]
    defs[name] = parse_type(body)
for name in ['BossOperation', 'EmployeeControl', 'MemoryOperation', 'AutomationOperation', 'PersonaDefaultAction', 'ComputerUseRunRequest']:
    load(name)
while refs - set(defs):
    load(sorted(refs - set(defs))[0])
source = pathlib.Path('crates/waku-protocol/src/boss.rs').read_text()

def patch_obj(schema, fragment):
    fragment = re.sub('///[^\\n]*', '', fragment)
    for k, v in schema.get('properties', {}).items():
        snake = re.sub('([A-Z])', lambda m: '_' + m[1].lower(), k)
        m = re.search('((?:#\\[[^\\]]*\\]\\s*)*)\\s*(?:pub\\s+)?' + snake + '\\s*:\\s*([^,]+),', fragment)
        if m:
            ty = m[2].strip()
            if 'Uuid' in ty:
                node = v.get('items', v)
                for child in node.get('anyOf', [node]):
                    if child.get('type') == 'string':
                        child['format'] = 'uuid'
            integer = re.search('\\bu(8|16|32|64)\\b', ty)
            if integer:
                for child in v.get('anyOf', [v]):
                    if child.get('type') == 'integer':
                        child.update(minimum=0, maximum=2 ** int(integer[1]) - 1)
        if m and ('default' in m[1] or m[2].strip().startswith('Option<')):
            if k in schema['required']:
                schema['required'].remove(k)
            if 'default' in m[1]:
                ty = m[2].strip()
                if ty == 'bool':
                    v['default'] = 'default_true' in m[1]
                elif ty.startswith('Vec<'):
                    v['default'] = []
                elif ty == 'String':
                    v['default'] = ''
                elif ty.startswith('BTreeMap<'):
                    v['default'] = {}
                elif re.fullmatch('u[0-9]+', ty):
                    v['default'] = 0
            if m[2].strip().startswith('u') and v.get('type') == 'integer':
                v['minimum'] = 0
for name in ['BossOperation', 'EmployeeControl', 'MemoryOperation', 'PersonaDefaultAction']:
    schema = defs[name]
    for variant in schema.get('anyOf', []):
        tag = variant.get('properties', {}).get('type', {}).get('const')
        if not tag:
            continue
        rust = tag[0].upper() + tag[1:]
        m = re.search('\\b' + rust + '\\s*\\{', source)
        if m:
            i = m.end()
            depth = 1
            j = i
            while depth:
                if source[j] == '{':
                    depth += 1
                if source[j] == '}':
                    depth -= 1
                j += 1
            patch_obj(variant, source[i:j - 1])
for variant in defs['BossOperation']['anyOf']:
    if variant['properties']['type']['const'] == 'summon':
        variant['properties']['workGoal']['default'] = 'errand'
for name, schema in defs.items():
    if schema.get('type') != 'object':
        continue
    for file in pathlib.Path('crates/waku-protocol/src').glob('*.rs'):
        raw = file.read_text()
        m = re.search('pub struct ' + name + '\\s*\\{', raw)
        if m:
            i = m.end()
            depth = 1
            j = i
            while depth:
                if raw[j] == '{':
                    depth += 1
                if raw[j] == '}':
                    depth -= 1
                j += 1
            patch_obj(schema, raw[i:j - 1])
            decl = raw[max(0, raw.rfind('#[derive', 0, m.start())):m.start()]
            schema['additionalProperties'] = 'deny_unknown_fields' not in decl
            break
load('BossResult')

def result_types(value):
    if isinstance(value, dict):
        if '$ref' in value and value['$ref'].split('/')[-1] not in defs:
            return {'protocolType': value['$ref'].split('/')[-1]}
        return {key: result_types(child) for key, child in value.items()}
    if isinstance(value, list):
        return [result_types(child) for child in value]
    return value
defs['BossResult'] = result_types(defs['BossResult'])
target = pathlib.Path('crates/waku-agent/src/boss-inputs.json')
content = json.dumps({'definitions': defs}, indent=2) + '\n'
if '--check' in __import__('sys').argv:
    if target.read_text() != content:
        raise SystemExit('Boss input definitions are stale; run python3 scripts/generate-boss-schema.py')
else:
    target.write_text(content)
print('input definitions:', len(defs), 'Boss variants:', len(defs['BossOperation']['anyOf']))
