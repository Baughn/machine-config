import com.cleanroommc.flare.proto.FlareSamplerProtos.SamplerData;
import com.cleanroommc.flare.proto.FlareSamplerProtos.ThreadNode;
import com.cleanroommc.flare.proto.FlareSamplerProtos.StackTraceNode;
import java.nio.file.*;
import java.util.*;
import java.io.*;

/** Decode Flare 0.8.0 profiles using the protobuf classes bundled in its JAR. */
class ProfileSummary {
    static String clean(Object value) {
        return value.toString().replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
    }
    static double sum(List<Double> times) {
        return times.stream().mapToDouble(Double::doubleValue).sum();
    }
    static void row(PrintWriter out, Object... values) {
        out.println(String.join("\t", Arrays.stream(values).map(ProfileSummary::clean).toList()));
    }
    static PrintWriter writer(Path dir, String name) throws IOException {
        return new PrintWriter(Files.newBufferedWriter(dir.resolve(name), StandardOpenOption.CREATE_NEW));
    }
    public static void main(String[] args) throws Exception {
        if (args.length != 2) throw new IllegalArgumentException("Usage: ProfileSummary.java PROFILE NEW_OUTPUT_DIR");
        SamplerData data;
        try (var input = Files.newInputStream(Path.of(args[0]))) {
            data = SamplerData.parseFrom(input);
        }
        if (!data.hasMetadata()) throw new IOException("No Flare sampler metadata; wrong format or empty profile");
        var meta = data.getMetadata();
        if (meta.getSamplerModeValue() != 0) throw new IOException("Execution profiles only; allocation weights have different units");
        Path dir = Path.of(args[1]);
        Files.createDirectory(dir);
        try (var out = writer(dir, "metadata.txt")) {
            row(out, "start_epoch_ms", meta.getStartTime());
            row(out, "end_epoch_ms", meta.getEndTime());
            row(out, "interval_us", meta.getInterval());
            row(out, "ticks", meta.getNumberOfTicks());
            row(out, "aggregator", meta.getDataAggregator().getType());
            row(out, "included_ticks", meta.getDataAggregator().getNumberOfIncludedTicks());
            row(out, "tick_threshold", meta.getDataAggregator().getTickLengthThreshold());
            row(out, "platform", meta.getPlatformMetadata());
            row(out, "java_version", meta.getSystemStatistics().getJava().getVersion());
            row(out, "thread_count", data.getThreadsCount());
            row(out, "time_windows", data.getTimeWindowsList());
            out.println("Weights are sampled execution milliseconds, not measured per-tick costs. Inclusive rows overlap.");
        }
        try (var out = writer(dir, "platform.txt")) {
            out.println(meta.getPlatformStatistics());
        }
        try (var out = writer(dir, "sources.tsv")) {
            row(out, "id", "name", "version");
            meta.getSourcesMap().forEach((id, source) -> row(out, id, source.getName(), source.getVersion()));
        }
        try (var out = writer(dir, "windows.tsv")) {
            row(out, "id", "start_epoch_ms", "end_epoch_ms", "ticks", "tps", "mspt_median", "mspt_max", "players", "entities", "tile_entities", "chunks", "cpu_process", "cpu_system");
            new TreeMap<>(data.getTimeWindowStatisticsMap()).forEach((id, w) -> row(out, id,
                w.getStartTime(), w.getEndTime(), w.getTicks(), w.getTps(), w.getMsptMedian(),
                w.getMsptMax(), w.getPlayers(), w.getEntities(), w.getTileEntities(),
                w.getChunks(), w.getCpuProcess(), w.getCpuSystem()));
        }
        try (var out = writer(dir, "frames.tsv"); var roots = writer(dir, "threads.tsv")) {
            row(roots, "thread_index", "thread", "weight_ms", "root_refs");
            row(out, "thread_index", "thread", "node_id", "inclusive_ms", "self_ms", "inclusive_pct_thread", "class", "method", "descriptor", "line", "source", "children_refs", "window_weights_ms");
            int threadId = 0;
            for (ThreadNode thread : data.getThreadsList()) {
                double total = sum(thread.getTimesList());
                row(roots, threadId, thread.getName(), total, thread.getChildrenRefsList());
                for (int ref : thread.getChildrenRefsList()) {
                    if (ref < 0 || ref >= thread.getChildrenCount()) throw new IOException("Invalid root reference");
                }
                for (int i = 0; i < thread.getChildrenCount(); i++) {
                    StackTraceNode node = thread.getChildren(i);
                    double inclusive = sum(node.getTimesList());
                    double children = 0;
                    for (int ref : node.getChildrenRefsList()) {
                        if (ref < 0 || ref >= thread.getChildrenCount()) throw new IOException("Invalid child reference");
                        children += sum(thread.getChildren(ref).getTimesList());
                    }
                    row(out, threadId, thread.getName(), i, inclusive, inclusive - children,
                        total > 0 ? 100 * inclusive / total : Double.NaN,
                        node.getClassName(), node.getMethodName(), node.getMethodDesc(), node.getLineNumber(),
                        data.getClassSourcesMap().getOrDefault(node.getClassName(), ""),
                        node.getChildrenRefsList(), node.getTimesList());
                }
                threadId++;
            }
        }
        System.out.println("Decoded " + data.getThreadsCount() + " threads to " + dir);
    }
}
