#!/usr/bin/env python3
"""Compare resident-world observations without confusing missing data with removal."""
import argparse
import json
from pathlib import Path


def compare(before, after):
    for report in (before, after):
        if report.get('schema') != 1 or report.get('error'):
            raise ValueError('Expected successful inspector schema 1 snapshots')
    if before.get('session') != after.get('session') or not before.get('session'):
        raise ValueError('Different/unknown server sessions; recapture after the restart')
    if before.get('query') != after.get('query') or not before.get('query', '').startswith(('chunk ', 'census')):
        raise ValueError('Compare identical chunk/census queries and offsets')
    elapsed = after['captured_epoch_ms'] - before['captured_epoch_ms']
    ticks = after['server_tick'] - before['server_tick']
    if elapsed <= 0 or ticks <= 0:
        raise ValueError('Need ordered snapshots separated by actual server ticks')
    complete = before.get('complete') is True and after.get('complete') is True
    result = {'schema': 1, 'session': before['session'], 'query': before['query'],
              'elapsed_seconds': elapsed / 1000, 'elapsed_server_ticks': ticks,
              'counts_comparable': complete, 'chunks': [], 'retained_items': []}

    def chunks(report):
        return {(world['dimension'], chunk['chunk_x'], chunk['chunk_z']): chunk
                for world in report['worlds'] for chunk in world['chunks']}
    old_chunks, new_chunks = chunks(before), chunks(after)
    for key in sorted(old_chunks.keys() | new_chunks.keys()):
        old, new = old_chunks.get(key, {}), new_chunks.get(key, {})
        result['chunks'].append({'dimension': key[0], 'chunk_x': key[1], 'chunk_z': key[2],
            'entity_delta': new.get('entities', 0) - old.get('entities', 0) if complete else None,
            'item_entity_delta': new.get('item_entities', 0) - old.get('item_entities', 0) if complete else None,
            'item_unit_delta': new.get('item_units', 0) - old.get('item_units', 0) if complete else None,
            'before': old, 'after': new})

    def items(report):
        return {(world['dimension'], entity['uuid']): entity
                for world in report['worlds'] for entity in world['entities'] if 'age_ticks' in entity}
    old_items, new_items = items(before), items(after)
    detail_complete = complete and all(w.get('details_complete') is True and w.get('entity_offset') == 0
                                      for r in (before, after) for w in r['worlds'])
    result['membership_comparable'] = detail_complete and before['query'].startswith('chunk ')
    result['new_item_uuids'] = [list(k) for k in sorted(new_items.keys() - old_items.keys())] if result['membership_comparable'] else None
    result['missing_item_uuids'] = [list(k) for k in sorted(old_items.keys() - new_items.keys())] if result['membership_comparable'] else None
    for key in sorted(old_items.keys() & new_items.keys()):
        old, new = old_items[key], new_items[key]
        age_delta = new['age_ticks'] - old['age_ticks']
        tick_delta = new['ticks_existed'] - old['ticks_existed']
        if new['age_ticks'] == -32768:
            finding = 'never-despawn age sentinel; unchanged age does not imply a tick failure'
        elif new.get('dead'):
            finding = 'marked dead at second observation'
        elif age_delta == 0 and tick_delta == 0:
            finding = 'same resident UUID without age or entity-tick advancement'
        elif age_delta == 0:
            finding = 'entity ticks advanced but item age did not; inspect item logic/update blocking/age resets'
        elif age_delta < 0:
            finding = 'age decreased; merging or custom age resets can explain this'
        else:
            finding = 'age advanced between observations'
        result['retained_items'].append({'dimension': key[0], 'uuid': key[1],
            'age_delta': age_delta, 'entity_tick_delta': tick_delta,
            'stack_count_delta': new['item_count'] - old['item_count'], 'finding': finding,
            'before': old, 'after': new})
    result['limitations'] = [
        'Two snapshots do not prove continuous residency, a constant chunk gate, or that an item never aged/reset between observations.',
        'A missing UUID may have moved, merged, unloaded, despawned, or been collected. New UUIDs do not prove a machine spawned them.',
        'Registry ID/damage are not full NBT/capability identity. Counts are not measured tick costs.',
    ]
    if not complete:
        result['limitations'].append('A scan was partial; totals/deltas and missing identities are inconclusive. Narrow the query.')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    args = parser.parse_args()
    try:
        result = compare(json.loads(args.before.read_text()), json.loads(args.after.read_text()))
    except (ValueError, KeyError) as exc:
        parser.error(str(exc))
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
