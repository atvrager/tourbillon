#!/usr/bin/env python3
"""Convert Verilog hex (objcopy -O verilog) to word-addressed $readmemh format.

Rebases addresses: the lowest @-address seen becomes word index 0.
This maps e.g. 0x80000000-based ELF segments to a zero-based BRAM init file.
"""
import sys

def convert(infile, outfile):
    addr = 0
    base = None
    words = {}
    with open(infile) as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            if line.startswith('@'):
                addr = int(line[1:], 16)
                if base is None:
                    base = addr
                continue
            for b in line.split():
                byte_val = int(b, 16)
                rebased = addr - (base or 0)
                word_idx = rebased // 4
                byte_pos = rebased % 4
                if word_idx not in words:
                    words[word_idx] = 0
                words[word_idx] |= byte_val << (8 * byte_pos)
                addr += 1

    if not words:
        return
    mx = max(words.keys())
    with open(outfile, 'w') as f:
        for i in range(mx + 1):
            f.write('%08x\n' % words.get(i, 0))

if __name__ == '__main__':
    convert(sys.argv[1], sys.argv[2])
