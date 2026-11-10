#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
PyInstaller 打包入口脚本。

通过子命令分发到不同功能：
  engine.exe serve --engine qwen3-asr-0.6b → 启动 Qwen3-ASR 0.6B Q8 服务器
  engine.exe serve --engine confucius4-r2t2 → 启动 R2T2 Q8 原生流式服务器
"""

import sys
import os
import argparse


def _setup_frozen_paths():
    """PyInstaller frozen 环境下，将 _internal/ 加入 sys.path"""
    if getattr(sys, "frozen", False):
        base = os.path.dirname(sys.executable)
        internal = os.path.join(base, "_internal")
        if os.path.isdir(internal) and internal not in sys.path:
            sys.path.insert(0, internal)


def cmd_serve(engine: str):
    from hf_cache_utils import TRANSCRIBE_CPP_ASR_MODELS

    if engine == "confucius4-r2t2":
        from r2t2_asr_server import R2T2ASRServer
        server = R2T2ASRServer()
    elif engine in TRANSCRIBE_CPP_ASR_MODELS or (
        engine.startswith("custom:")
        and os.environ.get("SPEAK_FLOW_CUSTOM_MODEL_PATH", "").strip()
    ):
        from qwen3_asr_server import Qwen3ASRServer
        server = Qwen3ASRServer(engine=engine)
    else:
        raise ValueError(f"Unsupported local ASR engine: {engine}")
    server.run()


def cmd_download(engine: str):
    from download_models import main
    main(engine=engine)


def main():
    _setup_frozen_paths()

    parser = argparse.ArgumentParser(prog="engine")
    sub = parser.add_subparsers(dest="command", required=True)

    from hf_cache_utils import LOCAL_ASR_MODELS

    serve_p = sub.add_parser("serve")
    engine_choices = sorted(LOCAL_ASR_MODELS)
    serve_p.add_argument("--engine", required=True)

    dl_p = sub.add_parser("download")
    dl_p.add_argument("--engine", required=True, choices=engine_choices)

    args = parser.parse_args()

    if args.command == "serve":
        cmd_serve(args.engine)
    elif args.command == "download":
        cmd_download(args.engine)


if __name__ == "__main__":
    main()
