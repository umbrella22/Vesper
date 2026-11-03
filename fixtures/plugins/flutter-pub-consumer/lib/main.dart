import 'package:flutter/material.dart';
import 'package:vesper_player/vesper_player.dart';

void main() {
  runApp(const VesperPubConsumerApp());
}

class VesperPubConsumerApp extends StatelessWidget {
  const VesperPubConsumerApp({super.key});

  @override
  Widget build(BuildContext context) {
    final source = VesperPlayerSource.hls(
      uri:
          'https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_ts/master.m3u8',
      label: 'Hosted pub consumer',
    );
    return MaterialApp(
      home: Scaffold(
        appBar: AppBar(title: const Text('Vesper pub consumer')),
        body: Center(child: Text(source.label)),
      ),
    );
  }
}
