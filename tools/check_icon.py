"""실행 파일에 assets/icon.ico의 그림이 실제로 들어갔는지 확인한다.
ICO 안의 각 그림 데이터(가장 큰 것 포함)가 실행 파일 바이트에 그대로 있어야 한다."""
import struct, sys

def images(ico):
    data = open(ico, 'rb').read()
    _, _, count = struct.unpack_from('<HHH', data, 0)
    out = []
    for i in range(count):
        size, offset = struct.unpack_from('<II', data, 6 + 16 * i + 8)
        out.append(data[offset:offset + size])
    return out

def main():
    ico, exes = sys.argv[1], sys.argv[2:]
    pics = images(ico)
    bad = False
    for exe in exes:
        blob = open(exe, 'rb').read()
        found = sum(1 for p in pics if p in blob)
        print(f'{exe}: {found}/{len(pics)} icon images embedded')
        if found != len(pics):
            bad = True
    sys.exit(1 if bad else 0)

if __name__ == '__main__':
    main()
