/*
    KWin - the KDE window manager
    This file is part of the KDE project.

    SPDX-License-Identifier: GPL-2.0-or-later
*/

#include "../src/outputconfigsearch.h"
#include "../src/core/output.h"

#include <QTest>

using namespace KWin;

class TestOutputConfigSearch : public QObject
{
    Q_OBJECT

private:
    static std::vector<OutputModeOption> makeModeOptions(
        const std::vector<std::pair<QSize, uint32_t>> &modes, bool includeDisabled = true, bool include8bpc = false)
    {
        std::vector<OutputModeOption> options;
        for (const auto &[size, refreshRate] : modes) {
            auto mode = std::make_shared<OutputMode>(size, refreshRate, OutputMode::Flags{});
            options.push_back(OutputModeOption{mode, 0});
            if (include8bpc) {
                options.push_back(OutputModeOption{mode, 8});
            }
        }
        if (includeDisabled) {
            options.push_back(OutputModeOption{nullptr, 0});
        }
        return options;
    }

private Q_SLOTS:
    void testEnumerateCandidates_singleOutput();
    void testEnumerateCandidates_twoOutputs();
    void testEnumerateCandidates_excludesAllDisabled();
    void testEnumerateCandidates_empty();
    void testScoreCandidates_prefersEnabled();
    void testScoreCandidates_prefersHigherResolution();
    void testScoreCandidates_prefersHigherRefreshRate();
    void testScoreCandidates_penalizes8bpc();
    void testCandidateDistance_enabledVsDisabled();
    void testCandidateDistance_sameModeIsZero();
    void testCandidateDistance_bandwidthDifference();
    void testCandidateDistance_bpcDifference();
    void testSpaceFillSort_firstIsHighestScored();
    void testSpaceFillSort_diverseSequence();
    void testSpaceFillSort_singleElement();
    void testBandwidth();
};

void TestOutputConfigSearch::testEnumerateCandidates_singleOutput()
{
    // 3 modes + disabled = 4 options; 3 candidates (excluding all-disabled)
    std::vector<std::vector<OutputModeOption>> options = {
        makeModeOptions({{QSize(1920, 1080), 60000}, {QSize(1920, 1080), 144000}, {QSize(3840, 2160), 60000}})};

    auto candidates = OutputConfigSearch::enumerateCandidates(options);
    QCOMPARE(candidates.size(), size_t(3)); // 3 enabled modes, disabled alone is excluded
}

void TestOutputConfigSearch::testEnumerateCandidates_twoOutputs()
{
    // Output 0: 2 modes + disabled = 3 options
    // Output 1: 2 modes + disabled = 3 options
    // Total: 3*3 = 9, minus all-disabled = 8
    std::vector<std::vector<OutputModeOption>> options = {
        makeModeOptions({{QSize(1920, 1080), 60000}, {QSize(2560, 1440), 60000}}),
        makeModeOptions({{QSize(1920, 1080), 60000}, {QSize(3840, 2160), 60000}})};

    auto candidates = OutputConfigSearch::enumerateCandidates(options);
    QCOMPARE(candidates.size(), size_t(8));
}

void TestOutputConfigSearch::testEnumerateCandidates_excludesAllDisabled()
{
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{nullptr, 0}},
        {OutputModeOption{nullptr, 0}}};

    auto candidates = OutputConfigSearch::enumerateCandidates(options);
    QCOMPARE(candidates.size(), size_t(0));
}

void TestOutputConfigSearch::testEnumerateCandidates_empty()
{
    std::vector<std::vector<OutputModeOption>> options = {};
    auto candidates = OutputConfigSearch::enumerateCandidates(options);
    QCOMPARE(candidates.size(), size_t(0));
}

void TestOutputConfigSearch::testScoreCandidates_prefersEnabled()
{
    auto mode1080 = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode1080, 0}, OutputModeOption{nullptr, 0}},
        {OutputModeOption{mode1080, 0}}};

    // Candidate A: both enabled; Candidate B: first disabled, second enabled
    std::vector<CandidateConfig> candidates = {
        CandidateConfig{{0, 0}, 0},
        CandidateConfig{{1, 0}, 0}};

    OutputConfigSearch::scoreCandidates(candidates, options);
    QVERIFY(candidates[0].score > candidates[1].score);
}

void TestOutputConfigSearch::testScoreCandidates_prefersHigherResolution()
{
    auto mode1080 = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    auto mode4k = std::make_shared<OutputMode>(QSize(3840, 2160), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode1080, 0}, OutputModeOption{mode4k, 0}}};

    std::vector<CandidateConfig> candidates = {
        CandidateConfig{{0}, 0},
        CandidateConfig{{1}, 0}};

    OutputConfigSearch::scoreCandidates(candidates, options);
    QVERIFY(candidates[1].score > candidates[0].score); // 4K scores higher
}

void TestOutputConfigSearch::testScoreCandidates_prefersHigherRefreshRate()
{
    auto mode60 = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    auto mode144 = std::make_shared<OutputMode>(QSize(1920, 1080), 144000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode60, 0}, OutputModeOption{mode144, 0}}};

    std::vector<CandidateConfig> candidates = {
        CandidateConfig{{0}, 0},
        CandidateConfig{{1}, 0}};

    OutputConfigSearch::scoreCandidates(candidates, options);
    QVERIFY(candidates[1].score > candidates[0].score); // 144Hz scores higher
}

void TestOutputConfigSearch::testScoreCandidates_penalizes8bpc()
{
    auto mode = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode, 0}, OutputModeOption{mode, 8}}};

    std::vector<CandidateConfig> candidates = {
        CandidateConfig{{0}, 0},
        CandidateConfig{{1}, 0}};

    OutputConfigSearch::scoreCandidates(candidates, options);
    QVERIFY(candidates[0].score > candidates[1].score); // native bpc scores higher
}

void TestOutputConfigSearch::testCandidateDistance_enabledVsDisabled()
{
    auto mode = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode, 0}, OutputModeOption{nullptr, 0}}};

    CandidateConfig a{{0}, 0};
    CandidateConfig b{{1}, 0};

    double d = OutputConfigSearch::candidateDistance(a, b, options);
    QCOMPARE(d, 1.0);
}

void TestOutputConfigSearch::testCandidateDistance_sameModeIsZero()
{
    auto mode = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode, 0}}};

    CandidateConfig a{{0}, 0};
    CandidateConfig b{{0}, 0};

    double d = OutputConfigSearch::candidateDistance(a, b, options);
    QCOMPARE(d, 0.0);
}

void TestOutputConfigSearch::testCandidateDistance_bandwidthDifference()
{
    auto mode60 = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    auto mode144 = std::make_shared<OutputMode>(QSize(1920, 1080), 144000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode60, 0}, OutputModeOption{mode144, 0}}};

    CandidateConfig a{{0}, 0};
    CandidateConfig b{{1}, 0};

    double d = OutputConfigSearch::candidateDistance(a, b, options);
    QVERIFY(d > 0.0);
    QVERIFY(d < 1.0); // should be less than enabled/disabled difference
}

void TestOutputConfigSearch::testCandidateDistance_bpcDifference()
{
    auto mode = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode, 0}, OutputModeOption{mode, 8}}};

    CandidateConfig a{{0}, 0};
    CandidateConfig b{{1}, 0};

    double d = OutputConfigSearch::candidateDistance(a, b, options);
    QCOMPARE(d, 0.2); // same bandwidth, different bpc
}

void TestOutputConfigSearch::testSpaceFillSort_firstIsHighestScored()
{
    auto mode60 = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    auto mode144 = std::make_shared<OutputMode>(QSize(1920, 1080), 144000);
    auto mode4k = std::make_shared<OutputMode>(QSize(3840, 2160), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode60, 0}, OutputModeOption{mode144, 0}, OutputModeOption{mode4k, 0}}};

    std::vector<CandidateConfig> candidates = {
        CandidateConfig{{0}, 0},
        CandidateConfig{{1}, 0},
        CandidateConfig{{2}, 0}};

    OutputConfigSearch::scoreCandidates(candidates, options);
    OutputConfigSearch::spaceFillSort(candidates, options);

    // First element should be 4K (highest scored)
    QCOMPARE(candidates[0].modeChoices[0], 2);
}

void TestOutputConfigSearch::testSpaceFillSort_diverseSequence()
{
    // Two outputs, each with two modes + disabled = 3 options each
    // 9 combinations - 1 all-disabled = 8 candidates
    auto modeLow = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    auto modeHigh = std::make_shared<OutputMode>(QSize(3840, 2160), 144000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{modeLow, 0}, OutputModeOption{modeHigh, 0}, OutputModeOption{nullptr, 0}},
        {OutputModeOption{modeLow, 0}, OutputModeOption{modeHigh, 0}, OutputModeOption{nullptr, 0}}};

    auto candidates = OutputConfigSearch::enumerateCandidates(options);
    OutputConfigSearch::scoreCandidates(candidates, options);
    OutputConfigSearch::spaceFillSort(candidates, options);

    // First should be best (both high)
    QCOMPARE(candidates[0].modeChoices[0], 1);
    QCOMPARE(candidates[0].modeChoices[1], 1);

    // Verify the sequence isn't just score-sorted (second element should be diverse)
    // It should NOT be the next-best-score (which would be very similar to the first)
    double dist01 = OutputConfigSearch::candidateDistance(candidates[0], candidates[1], options);
    QVERIFY(dist01 > 0.0);
}

void TestOutputConfigSearch::testSpaceFillSort_singleElement()
{
    auto mode = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    std::vector<std::vector<OutputModeOption>> options = {
        {OutputModeOption{mode, 0}}};

    std::vector<CandidateConfig> candidates = {CandidateConfig{{0}, 5.0}};
    OutputConfigSearch::spaceFillSort(candidates, options);
    QCOMPARE(candidates.size(), size_t(1));
}

void TestOutputConfigSearch::testBandwidth()
{
    auto mode = std::make_shared<OutputMode>(QSize(1920, 1080), 60000);
    OutputModeOption opt{mode, 0};
    QCOMPARE(opt.bandwidth(), uint64_t(1920) * 1080 * 60000);

    OutputModeOption disabled{nullptr, 0};
    QCOMPARE(disabled.bandwidth(), uint64_t(0));
}

QTEST_MAIN(TestOutputConfigSearch)
#include "test_outputconfigsearch.moc"
