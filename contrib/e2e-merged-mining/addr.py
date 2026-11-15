"""Kaspa-family address encoding (cashaddr-style bech32 with a 40-bit checksum) for local test addresses.
Usage: python addr.py <prefix> <hex byte 00..ff>  ->  <prefix>:<address> for public key = 32 x that byte."""
import sys

CHARSET = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l'
GEN = [0x98f2bc8e61, 0x79b76d99e2, 0xf33e5fb3c4, 0xae2eabe2a8, 0x1e4f43e470]


def polymod(values):
    c = 1
    for d in values:
        c0 = c >> 35
        c = ((c & 0x07ffffffff) << 5) ^ d
        for i in range(5):
            if (c0 >> i) & 1:
                c ^= GEN[i]
    return c ^ 1


def conv8to5(data):
    acc, bits, out = 0, 0, []
    for b in data:
        acc = (acc << 8) | b
        bits += 8
        while bits >= 5:
            bits -= 5
            out.append((acc >> bits) & 31)
    if bits:
        out.append((acc << (5 - bits)) & 31)
    return out


def encode(prefix, version, payload):
    data = conv8to5(bytes([version]) + payload)
    pre = [ord(c) & 31 for c in prefix] + [0]
    checksum = polymod(pre + data + [0] * 8)
    cs = [(checksum >> 5 * (7 - i)) & 31 for i in range(8)]
    return prefix + ':' + ''.join(CHARSET[d] for d in data + cs)


def verify(address):
    prefix, rest = address.split(':')
    values = [CHARSET.index(c) for c in rest]
    return polymod([ord(c) & 31 for c in prefix] + [0] + values) == 0


# self-check against a real mainnet address used in Miningcore's tests
assert verify('kaspa:qp454ffvlmhjvcvrclwcazf25jn6wj7r9wxtrln5lt964n5rxmukvpxc9wayk')
assert not verify('kaspa:qp454ffvlmhjvcvrclwcazf25jn6wj7r9wxtrln5lt964n5rxmukvpxc9wayq')

if __name__ == '__main__':
    print(encode(sys.argv[1], 0, bytes([int(sys.argv[2], 16)]) * 32))
