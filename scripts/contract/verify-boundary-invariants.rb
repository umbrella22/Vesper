#!/usr/bin/env ruby
# frozen_string_literal: true

require "pathname"

ROOT = Pathname.new(__dir__).join("..", "..").expand_path

show_warnings = ARGV.include?("--warnings")
show_all_warnings = ARGV.include?("--all-warnings")

Issue = Struct.new(:severity, :path, :line, :message, keyword_init: true)

def read(path)
  ROOT.join(path).read
end

def lines(path)
  read(path).lines
end

def source_files(*roots, extensions:)
  roots.flat_map do |root|
    Dir.glob(ROOT.join(root, "**", "*")).select do |path|
      File.file?(path) && extensions.include?(File.extname(path))
    end
  end.map { |path| Pathname.new(path).relative_path_from(ROOT).to_s }
end

def add_issue(issues, severity, path, line, message)
  issues << Issue.new(
    severity: severity,
    path: path,
    line: line,
    message: message
  )
end

def kotlin_block(lines, start_index)
  start_line = lines[start_index]
  brace_index = start_line.index("{")
  return start_line unless brace_index

  block_lines = [start_line]
  depth = 1
  remainder = start_line[(brace_index + 1)..] || ""
  depth += remainder.count("{")
  depth -= remainder.count("}")
  line_index = start_index + 1
  while depth.positive? && line_index < lines.length
    block_lines << lines[line_index]
    depth += lines[line_index].count("{")
    depth -= lines[line_index].count("}")
    line_index += 1
  end
  block_lines.join
end

def kotlin_function_context(lines, catch_index)
  search_start = [catch_index - 180, 0].max
  function_start = catch_index.downto(search_start).find do |line_index|
    lines[line_index].match?(/\bfun\b/)
  end
  return "" unless function_start

  lines[function_start..catch_index].join
end

def kotlin_coroutine_or_suspend_context?(lines, catch_index)
  function_context = kotlin_function_context(lines, catch_index)
  local_start = [catch_index - 12, 0].max
  local_context = lines[local_start..catch_index].join
  context = "#{function_context}\n#{local_context}"
  context.match?(/\bsuspend\s+fun\b|\b(?:launch|async)\s*\{|withContext\s*\(|runInterruptible\s*\{|[A-Za-z0-9_]+Async\s*\(/)
end

def kotlin_catch_chain_handles_cancellation?(lines, catch_index)
  function_context = kotlin_function_context(lines, catch_index)
  return false unless function_context.include?("CancellationException")

  search_start = [catch_index - 80, 0].max
  lines[search_start...catch_index].join.match?(/catch\s*\([^)]*:\s*CancellationException\s*\)/)
end

issues = []

plugin_kind_path =
  "lib/flutter/vesper_player_platform_interface/lib/src/models/plugin_diagnostic_models.dart"
plugin_capability_path =
  "lib/flutter/vesper_player_platform_interface/lib/src/models/plugins/plugin_capability_models.dart"
plugin_capability_test_path =
  "lib/flutter/vesper_player_platform_interface/test/events_test.dart"

plugin_kind_text = read(plugin_kind_path)
unless plugin_kind_text.match?(/enum\s+VesperPluginCapabilityKind\s*\{[^}]*\bunknown\b/m)
  add_issue(
    issues,
    :fail,
    plugin_kind_path,
    nil,
    "VesperPluginCapabilityKind must expose an unknown case for forward-compatible capability unions."
  )
end

plugin_capability_text = read(plugin_capability_path)
unknown_ctor = plugin_capability_text[/const\s+VesperPluginCapability\._unknown.*?;/m]
if unknown_ctor.nil?
  add_issue(
    issues,
    :fail,
    plugin_capability_path,
    nil,
    "VesperPluginCapability._unknown must exist so unknown capability records do not map to concrete union branches."
  )
elsif !unknown_ctor.include?("kind = VesperPluginCapabilityKind.unknown")
  add_issue(
    issues,
    :fail,
    plugin_capability_path,
    plugin_capability_text.lines.take_while { |line| !line.include?("VesperPluginCapability._unknown") }.length + 1,
    "VesperPluginCapability._unknown must set kind to VesperPluginCapabilityKind.unknown, not decoder/frameProcessor/sourceNormalizer."
  )
end

unless read(plugin_capability_test_path).include?("VesperPluginCapabilityKind.unknown")
  add_issue(
    issues,
    :fail,
    plugin_capability_test_path,
    nil,
    "Flutter plugin diagnostic tests must assert unknown capability kind preservation."
  )
end

swift_sources = source_files(
  "lib/ios/VesperPlayerKit/Sources",
  extensions: [".swift"]
)
swift_sources.each do |path|
  lines(path).each_with_index do |line, index|
    next unless line.match?(/\bwait\s*\(\s*\)/)

    add_issue(
      issues,
      :fail,
      path,
      index + 1,
      "Swift production wait() calls must use a timeout or async API; unbounded waits can deadlock host/plugin boundaries."
    )
  end
end

kotlin_sources = source_files(
  "lib/android",
  "lib/flutter",
  extensions: [".kt"]
).reject { |path| path.include?("/src/test/") || path.include?("/androidTest/") }
kotlin_sources.each do |path|
  file_lines = lines(path)
  file_lines.each_with_index do |line, index|
    if show_all_warnings && line.include?("runCatching")
      add_issue(
        issues,
        :warn,
        path,
        index + 1,
        "Review runCatching at lifecycle/protocol boundaries; it may hide CancellationException or unsupported platform failures."
      )
    end
    catch_match = line.match(/catch\s*\(\s*([A-Za-z_]\w*|_)\s*:\s*(Exception|Throwable|RuntimeException)\s*\)/)
    if catch_match
      window_start = [index - 8, 0].max
      window_end = [index + 4, file_lines.length - 1].min
      window = file_lines[window_start..window_end].join
      catch_block = kotlin_block(file_lines, index)
      catch_variable = catch_match[1]
      next if window.include?("CancellationException")
      next if kotlin_catch_chain_handles_cancellation?(file_lines, index)
      next if catch_variable != "_" && catch_block.match?(/\bthrow\s+#{Regexp.escape(catch_variable)}\b/)
      next unless show_all_warnings || kotlin_coroutine_or_suspend_context?(file_lines, index)

      add_issue(
        issues,
        :warn,
        path,
        index + 1,
        "Exception catch near suspend/lifecycle code should usually handle CancellationException before mapping failures."
      )
    end
  end
end

rust_sources = source_files(
  "crates/ffi",
  "crates/plugin",
  extensions: [".rs"]
)
rust_sources.each do |path|
  file_lines = lines(path)
  file_lines.each_with_index do |line, index|
    next unless line.match?(/\.(remove|swap_remove|pop|take)\b/)
    next if path.include?("/tests/")
    next if line.match?(/\.take\s*\(\s*[^)\s]/)
    next if line.match?(/sessions\.remove\(handle\);/)

    window_start = [index - 6, 0].max
    window_end = [index + 10, file_lines.length - 1].min
    window = file_lines[window_start..window_end].join
    next unless window.match?(/release|close|dispose/i)
    next if window.match?(/release.*\?\s*;.*\.(remove|swap_remove)|release_result|Ok\(\(\)\)/m)
    next if window.match?(/registry lock|drop\(session\)|worker\.join|schedule_resource_cleanup/)
    next if window.match?(/pending_frames\s*\.insert\s*\(\s*handle\s*,\s*frame\s*\)/)
    next if window.match?(/rejected_frames_pending_cleanup\s*\.push\s*\(\s*frame\s*\)/)
    next if window.match?(/outputs\.push\s*\(\s*output\s*\)/)
    next if window.match?(/leased_packet\s*=\s*Some\s*\(\s*packet\s*\)/)

    add_issue(
      issues,
      :warn,
      path,
      index + 1,
      "Review release ownership ordering; tracked resources should be removed only after fallible release succeeds."
    )
  end
end

ios_utility_queue = "lib/ios/VesperPlayerKit/Sources/VesperPlayerKit/NativePlayer/VesperBoundedUtilityQueue.swift"
if File.file?(ROOT.join(ios_utility_queue))
  lines(ios_utility_queue).each_with_index do |line, index|
    next unless line.include?("DispatchQueue.global")

    add_issue(
      issues,
      :warn,
      ios_utility_queue,
      index + 1,
      "Review bounded queue emergency fallback; required cleanup should stay bounded or document its escape hatch."
    )
  end
end

if show_warnings || show_all_warnings
  issues.each do |issue|
    next if issue.severity == :warn && !show_warnings && !show_all_warnings

    location = issue.line ? "#{issue.path}:#{issue.line}" : issue.path
    puts "#{issue.severity.to_s.upcase}: #{location}: #{issue.message}"
  end
end

failures = issues.select { |issue| issue.severity == :fail }
if failures.any?
  failures.each do |issue|
    location = issue.line ? "#{issue.path}:#{issue.line}" : issue.path
    warn "Boundary invariant failed: #{location}: #{issue.message}"
  end
  exit(1)
end

warning_count = issues.count { |issue| issue.severity == :warn }
if show_warnings
  puts "Boundary invariant scan passed with #{warning_count} warning candidate#{warning_count == 1 ? '' : 's'}."
elsif show_all_warnings
  puts "Boundary invariant scan passed with #{warning_count} warning candidate#{warning_count == 1 ? '' : 's'}."
else
  puts "Boundary invariant scan passed. Re-run with --warnings to inspect #{warning_count} focused warning candidate#{warning_count == 1 ? '' : 's'}, or --all-warnings for the broad scan."
end
